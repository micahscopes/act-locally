use crate::message::{Message, MessageDowncast, Response};
use crate::types::ActorError;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use std::any::Any;
// use std::cell::RefCell;
// use std::rc::Rc;
use std::future::Future;

use smol::lock::RwLock;
use std::sync::Arc;

pub trait AsyncMutatingFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncMutatingFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

pub trait AsyncFunc<'a, S, C, R, E>: Fn(&'a S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

type BoxAsyncMutatingFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

pub(crate) struct AsyncMutatingHandler<S, C, R> {
    func: BoxAsyncMutatingFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncMutatingHandler<S, C, R> {
    pub fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncMutatingFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncMutatingHandler {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

type BoxAsyncFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

pub(crate) struct AsyncHandler<S, C, R> {
    func: BoxAsyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncHandler<S, C, R> {
    pub fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncHandler {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

pub trait SyncMutatingFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncMutatingFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

pub trait SyncFunc<'a, S, C, R, E>: Fn(&'a S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

type BoxSyncMutatingFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> Result<R, ActorError> + Send + Sync>;

pub(crate) struct SyncMutatingHandler<S, C, R> {
    func: BoxSyncMutatingFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncMutatingHandler<S, C, R> {
    pub fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncMutatingFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncMutatingHandler {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

type BoxSyncFunc<S, C, R> = Box<dyn for<'a> Fn(&'a S, C) -> Result<R, ActorError> + Send + Sync>;

pub(crate) struct SyncHandler<S, C, R> {
    func: BoxSyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncHandler<S, C, R> {
    pub fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncHandler {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

pub(crate) trait AsyncMessageHandler<S>: Send + Sync {
    fn handle<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Response>, ActorError>>;
}

#[allow(unused)]
pub(crate) trait SyncMessageHandler<S>: Send + Sync {
    fn handle_sync<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> Result<Box<dyn Any + Send>, ActorError>;
}

impl<S, C, R> AsyncMessageHandler<S> for AsyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Response>, ActorError>> {
        if let Ok(params) = message.downcast::<C>() {
            Box::pin(async move {
                let mut state = state.write().await;
                let result = (self.func)(&mut *state, *params);
                result.await.map(|r| Box::new(r) as Box<dyn Response>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> AsyncMessageHandler<S> for AsyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Response>, ActorError>> {
        if let Ok(params) = message.downcast::<C>() {
            Box::pin(async move {
                let state = state.read().await;
                let result = (self.func)(&*state, *params);
                result.await.map(|r| Box::new(r) as Box<dyn Response>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> AsyncMessageHandler<S> for SyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Response>, ActorError>> {
        if let Ok(params) = message.downcast::<C>() {
            Box::pin(async move {
                let result = (self.func)(&mut *state.write().await, *params);
                result.map(|r| Box::new(r) as Box<dyn Response>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> SyncMessageHandler<S> for SyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle_sync<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Ok(params) = message.downcast::<C>() {
            let mut state = state.write_blocking();
            (self.func)(&mut *state, *params).map(|r| Box::new(r) as Box<dyn Any + Send>)
        } else {
            Err(ActorError::DispatchError)
        }
    }
}

impl<S, C, R> AsyncMessageHandler<S> for SyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Response>, ActorError>> {
        if let Ok(params) = message.downcast::<C>() {
            Box::pin(async move {
                let result = (self.func)(&*state.read().await, *params);
                result.map(|r| Box::new(r) as Box<dyn Response>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> SyncMessageHandler<S> for SyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send + Any,
{
    fn handle_sync<'a>(
        &'a self,
        state: Arc<RwLock<S>>,
        message: Box<dyn Message>,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Ok(params) = message.downcast::<C>() {
            (self.func)(&*state.read_blocking(), *params)
                .map(|r| Box::new(r) as Box<dyn Any + Send>)
        } else {
            Err(ActorError::DispatchError)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::message::ResponseDowncast;

    use super::*;
    use futures::executor::block_on;

    #[test]
    fn test_async_non_mutating_message_handler() {
        let state = Arc::new(RwLock::new(0u64));
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result =
            block_on(handler.handle(state.clone(), message)).expect("Failed to handle message");

        let result = result.downcast_ref::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_async_mutating_message_handler() {
        let state = Arc::new(RwLock::new(0u64));
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncMutatingHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result =
            block_on(handler.handle(state.clone(), message)).expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_non_mutating_message_handler() {
        let state = Arc::new(RwLock::new(0u64));
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncHandler::new(handler_fn);

        let message: Box<dyn Message> = Box::new(10u64);
        let result =
            block_on(handler.handle(state.clone(), message)).expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_mutating_message_handler() {
        let state = Arc::new(RwLock::new(0u64));
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncMutatingHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result =
            block_on(handler.handle(state.clone(), message)).expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_store_handlers_in_hashmap() {
        use std::collections::HashMap;

        let state = Arc::new(RwLock::new(0u64));

        async fn async_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        async fn async_handler_immut(s: &u64, _c: u64) -> Result<u64, ActorError> {
            Ok(*s)
        }

        fn sync_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        fn sync_handler_immut(s: &u64, _c: u64) -> Result<u64, ActorError> {
            Ok(*s)
        }

        let mut handlers: HashMap<&str, Box<dyn AsyncMessageHandler<u64>>> = HashMap::new();

        handlers.insert(
            "async_mut",
            Box::new(AsyncMutatingHandler::new(async_handler_mut)),
        );
        handlers.insert(
            "async_immut",
            Box::new(AsyncHandler::new(async_handler_immut)),
        );
        handlers.insert(
            "sync_mut",
            Box::new(SyncMutatingHandler::new(sync_handler_mut)),
        );
        handlers.insert("sync_immut", Box::new(SyncHandler::new(sync_handler_immut)));

        let message = Box::new(10u64);

        let result = block_on(handlers["async_mut"].handle(state.clone(), message))
            .expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let message = Box::new(10u64);
        let result = block_on(handlers["async_immut"].handle(state.clone(), message))
            .expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let message = Box::new(10u64);
        let result = block_on(handlers["sync_mut"].handle(state.clone(), message))
            .expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 20);

        let message = Box::new(10u64);
        let result = block_on(handlers["sync_immut"].handle(state.clone(), message))
            .expect("Failed to handle message");
        let result = result.downcast::<u64>().unwrap();
        assert_eq!(*result, 20);
    }

    #[test]
    fn test_store_sync_handlers_in_hashmap() {
        use std::collections::HashMap;

        let state = Arc::new(RwLock::new(0u64));

        fn sync_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        fn sync_handler_immut(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let mut handlers: HashMap<&str, Box<dyn SyncMessageHandler<u64>>> = HashMap::new();

        handlers.insert(
            "sync_mut",
            Box::new(SyncMutatingHandler::new(sync_handler_mut)),
        );
        handlers.insert("sync_immut", Box::new(SyncHandler::new(sync_handler_immut)));

        let message = Box::new(10u64);
        let result = handlers["sync_mut"].handle_sync(state.clone(), message);
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let message = Box::new(10u64);
        let result = handlers["sync_immut"].handle_sync(state.clone(), message);
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);
    }

    #[test]
    fn test_async_non_mutating_func_wrapper() {
        let state = 0;
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncHandler::new(handler_fn);

        let result = block_on((handler.func)(&state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_async_mutating_func_wrapper() {
        let mut state = 0;
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncMutatingHandler::new(handler_fn);

        let result = block_on((handler.func)(&mut state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_non_mutating_func_wrapper() {
        let state = 0;
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncHandler::new(handler_fn);

        let result = (handler.func)(&state, 10);
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_mutating_func_wrapper() {
        let mut state = 0;
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncMutatingHandler::new(handler_fn);

        let result = (handler.func)(&mut state, 10);
        assert_eq!(result.unwrap(), 10);
    }
}
