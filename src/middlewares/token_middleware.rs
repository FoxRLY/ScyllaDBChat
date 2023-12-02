use actix_web::{
    self,
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, HttpResponse,
};
use jsonwebtoken::jwk;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde_json;
use std::{
    collections::HashMap,
    env,
    future::{ready, Future, Ready},
    pin::Pin,
};

// .wrap_fn(|req, srv| {
//     let fut = srv.call(req);
//     async {
//         let res = fut.await?;
//         let (req, res) = res.into_parts();
//         let (res, body) = res.into_parts();
//
//         let body_bytes = body.try_into_bytes().unwrap();
//         let mut body_string = String::from_utf8(body_bytes.into()).unwrap();
//         println!("Intercepted {body_string}");
//         body_string.push_str(" bruh");
//         let res = res.set_body(body_string);
//         Ok(ServiceResponse::new(req, res))
// }})

pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthMiddlewareInner<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareInner { service }))
    }
}

pub struct AuthMiddlewareInner<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareInner<S>
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
        let user_id: i64;

        let token = if let Some(t) = req.cookie("token") {
            t
        } else {
            let (req, _req_body) = req.into_parts();
            let response = HttpResponse::PermanentRedirect()
                .insert_header(("Location", "/login"))
                .finish()
                .map_into_right_body();
            return Box::pin(async move { Ok(ServiceResponse::new(req, response)) });
        };
        let token = token.value();
        let jwk: jwk::Jwk =
            serde_json::from_str(&env::var("JWK").expect("JWK is not valid")).unwrap();
        match &jwk.algorithm {
            jwk::AlgorithmParameters::RSA(rsa) => {
                let key =
                    DecodingKey::from_rsa_components(&rsa.n, &rsa.e).expect("RSA key is not valid");
                let validation = Validation::new(jwk.common.algorithm.unwrap());
                let decoded_token =
                    decode::<HashMap<String, serde_json::Value>>(token, &key, &validation);
                if let Ok(token) = decoded_token {
                    user_id = token
                        .claims
                        .get("user_id")
                        .expect("user_id field is not present in JWT")
                        .as_i64()
                        .expect("user_id field is not i64 convertable");
                } else {
                    let (req, _req_body) = req.into_parts();
                    let response = HttpResponse::PermanentRedirect()
                        .insert_header(("Location", "/login"))
                        .finish()
                        .map_into_right_body();
                    return Box::pin(async move { Ok(ServiceResponse::new(req, response)) });
                }
            }
            _ => unreachable!("should be rsa"),
        }

        req.extensions_mut().insert(user_id);

        let res = self.service.call(req);
        Box::pin(async move {
            let res = res.await?;
            Ok(res.map_into_left_body())
        })
    }
}
