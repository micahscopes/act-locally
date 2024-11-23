use std::fmt::Debug;
use std::{any::Any, hash::Hash};

use crate::types::{ActorError, BoxedAny};

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct MessageKey<K>(pub K)
where
    K: Eq + Hash + Debug;

impl<K> MessageKey<K>
where
    K: Eq + Hash + Debug,
{
    pub fn new(key: K) -> Self {
        MessageKey(key)
    }
}

pub enum ActorMessage<K>
where
    K: Eq + Hash + Debug,
{
    Notification(MessageKey<K>, Box<dyn Message>),
    Request(
        MessageKey<K>,
        Box<dyn Message>,
        futures::channel::oneshot::Sender<Result<Box<dyn Response>, ActorError>>,
    ),
}

impl<K> Debug for ActorMessage<K>
where
    K: Eq + Hash + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorMessage::Notification(key, _message) => f
                .debug_struct("Notification")
                .field("key", key)
                // .field("message", message)
                .finish(),
            ActorMessage::Request(key, _message, _) => f
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

#[cfg(test)]
mod tests {
    // use super::*;

    // #[derive(Debug, PartialEq)]
    // struct TestMessage {
    //     content: String,
    // }
}
