use actix_web::{
    self,
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, HttpResponse,
};
use std::{
    future::{ready, Future, Ready},
    pin::Pin,
};

pub struct TestAuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for TestAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = TestAuthMiddlewareInner<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TestAuthMiddlewareInner { service }))
    }
}

pub struct TestAuthMiddlewareInner<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for TestAuthMiddlewareInner<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let user_id = req
            .headers()
            .get("chat_user_id")
            .and_then(|header| header.to_str().ok())
            .and_then(|raw_value| raw_value.parse::<i64>().ok());
        let user_id = if let Some(id) = user_id {
            id
        } else {
            let (req, _req_body) = req.into_parts();
            let response = HttpResponse::Unauthorized().finish().map_into_right_body();
            // let response = HttpResponse::PermanentRedirect()
            //     .insert_header(("Location", "/login"))
            //     .finish()
            //     .map_into_right_body();
            return Box::pin(async move { Ok(ServiceResponse::new(req, response)) });
        };

        req.extensions_mut().insert(user_id);

        let res = self.service.call(req);
        Box::pin(async move {
            let res = res.await?;
            Ok(res.map_into_left_body())
        })
    }
}
