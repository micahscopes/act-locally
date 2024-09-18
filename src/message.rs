use std::any::Any;
use std::fmt::Debug;

use crate::types::{ActorError, BoxedAny};

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct MessageKey(pub String);

impl MessageKey {
    pub fn new(key: &str) -> Self {
        MessageKey(key.to_string())
    }
}

pub enum ActorMessage {
    Notification(MessageKey, Box<dyn Message>),
    Request(
        MessageKey,
        Box<dyn Message>,
        futures::channel::oneshot::Sender<Result<Box<dyn Response>, ActorError>>,
    ),
}

impl Debug for ActorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorMessage::Notification(key, message) => f
                .debug_struct("Notification")
                .field("key", key)
                // .field("message", message)
                .finish(),
            ActorMessage::Request(key, message, _) => f
                .debug_struct("Request")
                .field("key", key)
                // .field("message", message)
                .field("sender", &"Sender")
                .finish(),
        }
    }
}

pub trait Response: Any + Send {
    fn any_response(&self) -> &dyn Any;
    fn boxed_any_response(self: Box<Self>) -> BoxedAny;
}

impl<T: Any + Send> Response for T {
    fn any_response(&self) -> &dyn Any {
        self
    }
    fn boxed_any_response(self: Box<Self>) -> BoxedAny {
        self
    }
}

pub trait ResponseDowncast {
    fn downcast_ref<T: Any>(&self) -> Option<&T>;
    fn downcast<T: Any>(self: Box<Self>) -> Result<Box<T>, Box<(dyn Any + Send + 'static)>>;
    fn is<T: Any>(&self) -> bool;
}

impl ResponseDowncast for dyn Response {
    fn downcast_ref<R: Any>(&self) -> Option<&R> {
        self.any_response().downcast_ref::<R>()
    }
    fn downcast<R: Any>(self: Box<Self>) -> Result<Box<R>, Box<(dyn Any + Send + 'static)>> {
        self.boxed_any_response().downcast::<R>()
    }
    fn is<T: Any>(&self) -> bool {
        self.any_response().is::<T>()
    }
}

pub trait Message: Any + Send {
    fn any_message(&self) -> &dyn Any;
    fn boxed_any_message(self: Box<Self>) -> BoxedAny;
}
impl<T: Any + Send> Message for T {
    fn any_message(&self) -> &dyn Any {
        self
    }
    fn boxed_any_message(self: Box<Self>) -> BoxedAny {
        self
    }
}

pub trait MessageDowncast {
    fn downcast_ref<T: Any>(&self) -> Option<&T>;
    fn downcast<T: Any>(self: Box<Self>) -> Result<Box<T>, Box<(dyn Any + Send + 'static)>>;
    fn is<T: Any>(&self) -> bool;
}

impl MessageDowncast for dyn Message {
    fn downcast_ref<R: Any>(&self) -> Option<&R> {
        self.any_message().downcast_ref::<R>()
    }
    fn downcast<M: Any>(self: Box<Self>) -> Result<Box<M>, Box<(dyn Any + Send + 'static)>> {
        self.boxed_any_message().downcast::<M>()
    }
    fn is<T: Any>(&self) -> bool {
        self.any_message().is::<T>()
    }
}

pub struct BoxedMessage {
    inner: Box<dyn Message>,
}

impl BoxedMessage {
    pub fn new<T: Any + Send + 'static>(value: T) -> Self {
        BoxedMessage {
            inner: Box::new(value),
        }
    }

    pub fn downcast<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[derive(Debug, PartialEq)]
//     struct TestMessage {
//         content: String,
//     }

//     #[test]
//     fn test_boxed_message_downcast() {
//         let original = TestMessage {
//             content: "Hello, world!".to_string(),
//         };
//         let boxed = BoxedMessage::new(original);

//         if let Some(downcasted) = boxed.downcast_ref::<TestMessage>() {
//             assert_eq!(downcasted.content, "Hello, world!");
//         } else {
//             panic!("Failed to downcast BoxedMessage");
//         }
//     }

//     #[test]
//     fn test_boxed_message_failed_downcast() {
//         let original = TestMessage {
//             content: "Hello, world!".to_string(),
//         };
//         let boxed = BoxedMessage::new(original);

//         assert!(boxed.downcast_ref::<String>().is_none());
//     }

//     #[test]
//     fn test_message_trait_downcast() {
//         let message = TestMessage {
//             content: "Test content".to_string(),
//         };
//         let message_ref: &dyn Message = &message;

//         if let Some(downcasted) = MessageDowncast::downcast_ref::<TestMessage>(message_ref) {
//             assert_eq!(downcasted.content, "Test content");
//         } else {
//             panic!("Failed to downcast Message trait object");
//         }
//     }

//     #[test]
//     fn test_message_key() {
//         let key = MessageKey::new("test_key");
//         assert_eq!(key.0, "test_key");
//     }

//     #[test]
//     fn test_actor_message_notification() {
//         let key = MessageKey::new("notification");
//         let message = BoxedMessage::new(TestMessage {
//             content: "Notification content".to_string(),
//         });
//         let actor_message = ActorMessage::Notification(key, message);

//         match actor_message {
//             ActorMessage::Notification(k, m) => {
//                 assert_eq!(k.0, "notification");
//                 if let Some(test_message) = m.downcast_ref::<TestMessage>() {
//                     assert_eq!(test_message.content, "Notification content");
//                 } else {
//                     panic!("Failed to downcast notification message");
//                 }
//             }
//             _ => panic!("Expected Notification variant"),
//         }
//     }
// }
