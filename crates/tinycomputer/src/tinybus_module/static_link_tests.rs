//! The statically linked build is admitted by the in-process module host.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tinybus::broker::Broker;
use tinybus::module::ModuleHost;
use tinybus::transport::memory::MemoryBus;

#[tokio::test]
async fn linked_module_is_admitted_by_the_module_host() {
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let broker_task = broker.spawn(bus);
    let host = ModuleHost::new(broker);
    let module = super::linked_module().expect("manifest is valid");
    let bus_name = module.manifest.bus_name.clone();
    let info = host
        .attach_linked_with_config(module, serde_json::json!({}))
        .expect("linked tinycomputer is admitted");
    assert_eq!(info.manifest.bus_name, bus_name);
    assert_eq!(host.list().len(), 1);
    broker_task.abort();
}
