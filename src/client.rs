use std::time::Duration;

use anyhow::Result;
use iceoryx2::{
    port::{
        listener::Listener, notifier::Notifier, publisher::Publisher,
        update_connections::UpdateConnections,
    },
    prelude::*,
};

mod common;
mod events;

use common::SERVICE_NAME;
use events::IpcEvent;

const CYCLE_TIME: Duration = Duration::from_secs(1);
const HISTORY_SIZE: usize = 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];

    let node = NodeBuilder::new().create::<ipc::Service>()?;
    let service_name: ServiceName = SERVICE_NAME.try_into()?;
    let ipc_client = IpcClient::new(&node, &service_name)?;

    let waitset = WaitSetBuilder::new().create::<ipc::Service>()?;
    let publisher_guard = waitset.attach_notification(&ipc_client)?;
    let cyclic_trigger_guard = waitset.attach_interval(CYCLE_TIME)?;

    let mut sent = false;

    let on_event = |attachment_id: WaitSetAttachmentId<ipc::Service>| {
        if attachment_id.has_event_from(&cyclic_trigger_guard) {
            if !sent {
                println!("Send message");
                ipc_client.send(path).unwrap();
                sent = true;
            }
        } else if attachment_id.has_event_from(&publisher_guard) {
            ipc_client.handle_event().unwrap();
        }
        CallbackProgression::Continue
    };

    // Start the event loop. It will run until `CallbackProgression::Stop` is returned by the
    // event callback or an interrupt/termination signal was received.
    waitset.wait_and_process(on_event)?;

    println!("exit");
    Ok(())
}

#[derive(Debug)]
struct IpcClient {
    publisher: Publisher<ipc::Service, [u8], ()>,
    listener: Listener<ipc::Service>,
    notifier: Notifier<ipc::Service>,
}

impl FileDescriptorBased for IpcClient {
    fn file_descriptor(&self) -> &FileDescriptor {
        self.listener.file_descriptor()
    }
}

impl SynchronousMultiplexing for IpcClient {}

impl IpcClient {
    fn new(
        node: &Node<ipc::Service>,
        service_name: &ServiceName,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pubsub_service = node
            .service_builder(service_name)
            .publish_subscribe::<[u8]>()
            .history_size(HISTORY_SIZE)
            .subscriber_max_buffer_size(HISTORY_SIZE)
            .open_or_create()?;
        let event_service = node
            .service_builder(service_name)
            .event()
            .open_or_create()?;

        let listener = event_service.listener_builder().create()?;
        let notifier = event_service.notifier_builder().create()?;
        let publisher = pubsub_service
            .publisher_builder()
            .initial_max_slice_len(16)
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()?;

        notifier.notify_with_custom_event_id(IpcEvent::ClientConnected.into())?;

        Ok(Self {
            publisher,
            listener,
            notifier,
        })
    }

    fn handle_event(&self) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(event) = self.listener.try_wait_one()? {
            let event: IpcEvent = event.into();
            match event {
                IpcEvent::ServerConnected => {
                    println!("Server connected");
                    self.publisher.update_connections().unwrap();
                }
                IpcEvent::ServerDisconnected => {
                    println!("Server disconnected");
                }
                IpcEvent::RequestReceived => {
                    println!("Server has received request");
                }
                _ => (),
            }
        }

        Ok(())
    }

    fn send(&self, path: &str) -> Result<()> {
        let sample = self.publisher.loan_slice_uninit(path.len())?;
        let sample = sample.write_from_slice(path.as_bytes());
        sample.send()?;

        self.notifier
            .notify_with_custom_event_id(IpcEvent::RequestSent.into())?;
        Ok(())
    }
}

impl Drop for IpcClient {
    fn drop(&mut self) {
        let _ = self
            .notifier
            .notify_with_custom_event_id(IpcEvent::ClientDisconnected.into());
    }
}
