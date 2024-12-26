use iceoryx2::port::event_id::EventId;
use num_enum::TryFromPrimitive;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum IpcEvent {
    ServerConnected = 0,
    ServerDisconnected = 1,
    ClientConnected = 2,
    ClientDisconnected = 3,

    RequestSent = 4,
    RequestReceived = 5,
    ResponseSent = 6,
    ResponseReceived = 7,

    ProcessDied = 8,

    Unknown,
}

impl From<IpcEvent> for EventId {
    fn from(value: IpcEvent) -> Self {
        EventId::new(value as usize)
    }
}

impl From<EventId> for IpcEvent {
    fn from(value: EventId) -> Self {
        IpcEvent::try_from(value.as_value()).unwrap_or(IpcEvent::Unknown)
    }
}
