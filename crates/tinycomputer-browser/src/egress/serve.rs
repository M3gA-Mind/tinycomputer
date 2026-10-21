//! Authorizes requests and consumes grants before dialing approved addresses.

use std::{io, sync::Arc, time::Duration};

use tokio::net::TcpStream;

use super::{
    BrowserDialTarget, DialGrant, EgressAuthorizer,
    forward::{
        forward_response_with_lease, initialize_upstream, refuse, tunnel_with_lease, validate_lease,
    },
    request::{read_proxy_request, read_request_body},
};

pub(super) async fn serve(
    mut client: TcpStream,
    auth: Arc<dyn EgressAuthorizer>,
    session_id: String,
    proxy_password: String,
) -> io::Result<()> {
    let Some(mut request) = read_proxy_request(&mut client, &session_id, &proxy_password).await?
    else {
        return Ok(());
    };
    if !read_request_body(&mut client, &mut request).await? {
        return refuse(&mut client).await;
    }
    let Some(first_grant) = authorize(&auth, &request.target).await else {
        return refuse(&mut client).await;
    };
    let addresses = first_grant.addresses.clone();
    let mut connected = None;
    for (index, address) in addresses.into_iter().enumerate() {
        // A consume call spends its one-use grant. Reauthorize every later
        // candidate so a failed dial cannot reuse a spent authorization.
        let grant = if index == 0 {
            first_grant.clone()
        } else {
            match authorize(&auth, &request.target).await {
                Some(grant) if grant.addresses.contains(&address) => grant,
                _ => continue,
            }
        };
        if !matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                auth.consume(request.target.clone(), grant.clone(), address),
            )
            .await,
            Ok(Ok(()))
        ) {
            return refuse(&mut client).await;
        }
        if let Ok(Ok(upstream)) =
            tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(address)).await
        {
            connected = Some((upstream, grant.lease));
            break;
        }
    }
    let Some((mut upstream, lease)) = connected else {
        return refuse(&mut client).await;
    };
    if !validate_lease(&auth, &lease).await? {
        return Ok(());
    }
    initialize_upstream(&mut client, &mut upstream, &request).await?;
    if request.connect || request.has_upgrade {
        tunnel_with_lease(&mut client, &mut upstream, &auth, lease).await?;
    } else {
        forward_response_with_lease(&mut client, &mut upstream, &auth, lease).await?;
    }
    Ok(())
}

async fn authorize(
    auth: &Arc<dyn EgressAuthorizer>,
    target: &BrowserDialTarget,
) -> Option<DialGrant> {
    match tokio::time::timeout(Duration::from_secs(5), auth.authorize(target.clone())).await {
        Ok(Ok(grant)) if !grant.addresses.is_empty() && grant.addresses.len() <= 64 => Some(grant),
        _ => None,
    }
}
