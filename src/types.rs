use std::any::Any;
use std::boxed::Box;
use std::error::Error;
use std::fmt;

pub type BoxedAny = Box<dyn Any + Send>;

#[derive(Debug)]
pub enum ActorError {
    HandlerNotFound,
    CustomError(Box<dyn Error + Send + Sync>),
    SendError,
    DispatchError,
    DowncastError,
    HandlerPanicked,
    SpawnError(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorError::HandlerNotFound => write!(f, "Handler not found"),
            ActorError::CustomError(e) => write!(f, "Custom error: {}", e),
            ActorError::SendError => write!(f, "Failed to send message"),
            ActorError::DispatchError => write!(f, "Failed to dispatch message"),
            ActorError::DowncastError => write!(f, "Failed to downcast message"),
            ActorError::HandlerPanicked => write!(f, "Handler panicked"),
            ActorError::SpawnError(e) => write!(f, "Failed to spawn actor due to error: {}", e),
        }
    }
}

impl std::error::Error for ActorError {}
