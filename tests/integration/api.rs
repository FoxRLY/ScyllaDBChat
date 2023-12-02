use actix::Actor;
use actix_http::{
    body::{BoxBody, EitherBody},
    StatusCode,
};
use actix_service::Service;
use actix_web::dev::ServiceResponse;
use actix_web::{web, App};
use chat::{
    actors::{
        broker_actor::BrokerActor,
        database_actor::{self, DatabaseActor},
        redis_actor::RedisActor,
    },
    handlers::{
        add_user_to_chat,
        create_new_group_chat,
        create_new_private_chat,
        exit_chat,
        get_chat_info,
        get_user_info,
        get_user_chats,
        authorize_user,
        data_types::Addresses},
    middlewares::test_token_middleware::TestAuthMiddleware,
};
use serial_test::serial;
use urlencoding::encode;

macro_rules! uri {
    ($main_string:expr) => {
        format!($main_string)
    };
    (
        $main_string:expr,
        $($elem: expr),+
    ) => {
        format!($main_string, $( encode($elem) ),+ )
    };
    (
        $main_string:expr,
        $($elem: ident),+
    ) => {
        format!($main_string, $( encode($elem) ),+ )
    };
    (
        $main_string:tt,
        $($elem: expr),+
    ) => {
        format!($main_string, $( encode($elem) ),+ )
    };
}

async fn parse_response<T: Sized + serde::de::DeserializeOwned>
(
    response: ServiceResponse<EitherBody<BoxBody>>,
    success_code: StatusCode,
)
-> Result<T, (StatusCode, String)> {
    let status = response.status();
    let response_body = actix_web::test::read_body(response).await;
    let response_body = String::from_utf8(response_body.to_vec());
    if status != success_code || response_body.is_err() {
        Err((status, response_body.unwrap_or_else(|e| e.to_string())))
    } else {
        let body = response_body.unwrap_or_else(|e| e.to_string());
        let r = serde_json::from_str::<T>(&body).unwrap();
        Ok(r)
    }
}

async fn get_response_text
(
    response: ServiceResponse<EitherBody<BoxBody>>,
    success_code: StatusCode,
)
-> Result<String, (StatusCode, String)> {
    let status = response.status();
    let response_body = actix_web::test::read_body(response).await;
    let response_body = String::from_utf8(response_body.to_vec());
    if status != success_code || response_body.is_err() {
        Err((status, response_body.unwrap_or_else(|e| e.to_string())))
    } else {
        let body = response_body.unwrap_or_else(|e| e.to_string());
        Ok(body)
    }
}

mod api_tests {

    use chat::{database::data::{UserInfo, ChatInfo, ChatType}, handlers::data_types::UserInfoStripped};
    use uuid::Uuid;

    use super::*;

    fn create_new_user_request(user_name: &str, user_id: i64)-> actix_http::Request {
        let uri = uri!("/authorization?user_name={}", user_name);
        let request = actix_web::test::TestRequest::post()
            .uri(&uri)
            .insert_header(("chat_user_id", user_id))
            .to_request();
        request
    }
    
    fn get_user_chats_request(user_id: i64) -> actix_http::Request {
        let uri = uri!("/chats");
        let request = actix_web::test::TestRequest::get()
            .uri(&uri)
            .insert_header(("chat_user_id", user_id))
            .to_request();
        request
    }

    fn create_new_private_chat_request(creator_id: i64, chat_name: &str, guest_id: i64) -> actix_http::Request {
        let uri = uri!("/new-private?guest_user={}&new_chat_name={}", &guest_id.to_string(), chat_name);
        let req = actix_web::test::TestRequest::post()
            .uri(&uri)
            .insert_header(("chat_user_id", creator_id))
            .to_request();
        req
    }

    async fn prepare_database() -> web::Data<chat::handlers::data_types::Addresses> {
        let db = DatabaseActor::new("127.0.0.1".into(), 9042)
            .await
            .unwrap()
            .start();
        db.send(database_actor::messages::InitDatabaseClear)
            .await
            .unwrap()
            .unwrap();
        let broker = BrokerActor::new(db.clone()).await.start();
        let redis = RedisActor::new("127.0.0.1", 6379, broker.clone())
            .await
            .unwrap()
            .start();
        let addrs = Addresses {
            db: db.clone(),
            broker: broker.clone(),
            redis: redis.clone(),
        };
        let data = web::Data::new(addrs);
        data
    }

    #[actix_web::test]
    #[serial]
    async fn get_user_info_test(){
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_info)
                .service(create_new_private_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware)
        )
        .await;
        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let _r = app.call(create_new_private_chat_request(1, "Test chat", 2)).await.unwrap();
        let req = actix_web::test::TestRequest::get()
            .uri(&uri!("/info?user_id={}", "2"))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let res = app.call(req).await.unwrap();
        let result: UserInfoStripped = parse_response(res, StatusCode::OK).await.unwrap();
        assert_eq!(result.id, 2);
        assert_eq!(&result.name, "Test user 2");
    }

    #[actix_web::test]
    #[serial]
    async fn add_new_chat_member_test(){
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_chats)
                .service(create_new_private_chat)
                .service(add_user_to_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;

        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 3", 3)).await.unwrap();
        let res = app.call(create_new_private_chat_request(1, "Test chat", 2)).await.unwrap();
        let chat_info: ChatInfo = parse_response(res, StatusCode::OK).await.unwrap();
        let req = actix_web::test::TestRequest::put()
            .uri(&uri!("/new-user?guest_id={}&chat_id={}", &3.to_string(), &chat_info.id.to_string()))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let _res = app.call(req).await.unwrap();
        let res = app.call(get_user_chats_request(3)).await.unwrap();
        let user_chats: Vec<Uuid> = parse_response(res, StatusCode::OK).await.unwrap();
        assert!(user_chats.contains(&chat_info.id));
    }


    #[actix_web::test]
    #[serial]
    async fn get_chat_info_test(){
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_chat_info)
                .service(create_new_private_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let r = app.call(create_new_private_chat_request(1, "Test chat", 2)).await.unwrap();
        let chat_info: ChatInfo = parse_response(r, StatusCode::OK).await.unwrap();
        let req = actix_web::test::TestRequest::get()
            .uri(&uri!("/info?chat_id={}", &chat_info.id.to_string()))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let res = app.call(req).await.unwrap();
        let new_chat_info: ChatInfo = parse_response(res, StatusCode::OK).await.unwrap();
        assert_eq!(chat_info.id, new_chat_info.id);
        assert_eq!(chat_info.chat_type, new_chat_info.chat_type);
        assert_eq!(chat_info.name, new_chat_info.name);
        assert_eq!(chat_info.users, new_chat_info.users);
    }

    #[actix_web::test]
    #[serial]
    async fn exit_chat_test() {
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_chats)
                .service(create_new_private_chat)
                .service(exit_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let r = app.call(create_new_private_chat_request(1, "Test chat", 2)).await.unwrap();
        let chat_info: ChatInfo = parse_response(r, StatusCode::OK).await.unwrap();
        let req = actix_web::test::TestRequest::put()
            .uri(&uri!("/exit?chat_id={}", &chat_info.id.to_string()))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let _res = app.call(req).await.unwrap();
        let res = app.call(get_user_chats_request(1)).await.unwrap();
        let chats_raw = get_response_text(res, StatusCode::OK).await.unwrap();
        let chats: Vec<Uuid> = serde_json::from_str(&chats_raw).unwrap();
        assert!(chats.is_empty());
    }
    

    #[actix_web::test]
    #[serial]
    async fn create_group_chat_test() {
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_chats)
                .service(create_new_group_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 3", 3)).await.unwrap();
        let user_ids = serde_json::to_string(&vec!(2,3)).unwrap();
        let req = actix_web::test::TestRequest::post()
            .uri(&uri!("/new-group?guest_users={}&new_chat_name={}", &user_ids, "Test chat"))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let res = app.call(req).await.unwrap();
        let chat_info: ChatInfo = parse_response(res, StatusCode::OK).await.unwrap();
        assert_eq!(&chat_info.name, "Test chat");
        assert_eq!(chat_info.chat_type, ChatType::Group);
        assert!(chat_info.users.contains(&1));
        assert!(chat_info.users.contains(&2));
        assert!(chat_info.users.contains(&3));
        let res = app.call(get_user_chats_request(1)).await.unwrap();
        let user_info: Vec<Uuid> = parse_response(res, StatusCode::OK).await.unwrap();
        assert!(user_info.contains(&chat_info.id));
    }

    #[actix_web::test]
    #[serial]
    async fn create_private_chat_test() {
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_chats)
                .service(create_new_private_chat)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let _r = app.call(create_new_user_request("Test user 1", 1)).await.unwrap();
        let _r = app.call(create_new_user_request("Test user 2", 2)).await.unwrap();
        let req = actix_web::test::TestRequest::post()
            .uri(&uri!("/new-private?guest_user=2&new_chat_name={}", "Test chat"))
            .insert_header(("chat_user_id", 1))
            .to_request();
        let res = app.call(req).await.unwrap();
        let chat_info: ChatInfo = parse_response(res, StatusCode::OK).await.unwrap();
        assert_eq!(&chat_info.name, "Test chat");
        assert_eq!(chat_info.chat_type, ChatType::Private);
        assert!(chat_info.users.contains(&1));
        assert!(chat_info.users.contains(&2));
        let res = app.call(get_user_chats_request(1)).await.unwrap();
        let user_info: Vec<Uuid> = parse_response(res, StatusCode::OK).await.unwrap();
        assert!(user_info.contains(&chat_info.id));
    }

    #[actix_web::test]
    #[serial]
    async fn get_user_chats_empty_test() {
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .service(get_user_chats)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let new_user_request = create_new_user_request("Test user", 3);
        let response = app.call(new_user_request).await.unwrap();
        let _response = parse_response::<UserInfo>(response, StatusCode::OK).await.unwrap();
        let uri = uri!("/chats");
        let request = actix_web::test::TestRequest::get()
            .uri(&uri)
            .insert_header(("chat_user_id", 3))
            .to_request();
        let response = app.call(request).await.unwrap();
        let response = parse_response::<Vec<Uuid>>(response, StatusCode::OK).await.unwrap();
        assert!(response.is_empty())
    }

    #[actix_web::test]
    #[serial]
    async fn authorize_user_test() {
        let data = prepare_database().await;
        let app = actix_web::test::init_service(
            App::new()
                .service(authorize_user)
                .app_data(data)
                .wrap(TestAuthMiddleware),
        )
        .await;
        let uri = uri!("/authorization?user_name={}", "Test User");
        let request = actix_web::test::TestRequest::post()
            .uri(&uri)
            .insert_header(("chat_user_id", 3))
            .to_request();
        let response = app.call(request).await.unwrap();
        let response = parse_response::<UserInfo>(response, StatusCode::OK).await.unwrap();
        assert_eq!(response.id, 3);
        assert_eq!(&response.name, "Test User");
        assert!(response.chats.is_empty());
    }
}
