//! Failure cases for bounded chunked request framing.

use super::forward_chunked_request_body;

async fn forward(input: &[u8]) -> std::io::Result<Vec<u8>> {
    let (mut client_reader, client_writer) = tokio::io::duplex(input.len().max(1));
    let (mut upstream_writer, mut upstream_reader) = tokio::io::duplex(input.len().max(1));
    let input = input.to_vec();
    let write = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        let mut client_writer = client_writer;
        client_writer.write_all(&input).await?;
        Ok::<_, std::io::Error>(())
    });
    let result = forward_chunked_request_body(&mut client_reader, &mut upstream_writer, &[]).await;
    if result.is_ok() {
        use tokio::io::AsyncWriteExt as _;
        upstream_writer.shutdown().await?;
    }
    drop(client_reader);
    drop(upstream_writer);
    let mut output = Vec::new();
    use tokio::io::AsyncReadExt as _;
    upstream_reader.read_to_end(&mut output).await?;
    write.await.expect("client writer task should finish")?;
    result.map(|()| output)
}

#[tokio::test]
async fn rejects_invalid_chunk_size_forms() {
    for input in [
        &[0xff, b'\r', b'\n'][..],
        b"\r\n",
        b"not-hex\r\n",
        b"ffffffffffffffffffffffffffffffff\r\n",
    ] {
        assert!(forward(input).await.is_err());
    }
}

#[tokio::test]
async fn rejects_incomplete_chunks_and_invalid_chunk_endings() {
    assert!(forward(b"1\r\n").await.is_err());
    assert!(forward(b"1\r\nxNO").await.is_err());
}

#[tokio::test]
async fn rejects_invalid_and_forbidden_trailers() {
    assert!(forward(b"0\r\nmissing-colon\r\n\r\n").await.is_err());
    assert!(forward(b"0\r\nHost: forged.test\r\n\r\n").await.is_err());
    assert!(forward(b"0\r\nContent-Length: 1\r\n\r\n").await.is_err());
}

#[tokio::test]
async fn rejects_bytes_after_the_terminal_chunk() {
    assert!(forward(b"0\r\n\r\nextra").await.is_err());
}

#[tokio::test]
async fn rejects_chunk_lines_over_the_limit() {
    let mut input = vec![b'a'; 8193];
    input.extend_from_slice(b"\r\n");
    assert!(forward(&input).await.is_err());
}

#[tokio::test]
async fn forwards_valid_chunk_data_and_trailers_verbatim() {
    let input = b"4;ext=yes\r\ndata\r\n0\r\nX-Trace: ok\r\n\r\n";
    let forwarded = forward(input).await.unwrap();
    assert_eq!(forwarded, input);
}
