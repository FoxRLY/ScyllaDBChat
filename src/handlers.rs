use crate::{
    actors::{
        broker_actor::BrokerActor,
        database_actor::{self, DatabaseActor},
        redis_actor::RedisActor,
        websocket_actor::WebsocketActor,
    },
    database::{data::UserInfo, DBError},
};
use actix::Addr;
use actix_web::{
    self, get, post, put,
    web::{self, ReqData},
    HttpRequest, HttpResponse, Responder,
};
use actix_web_actors::ws;
use uuid::Uuid;

pub mod data_types {
    use crate::database::PageIndex;

    use super::*;
    pub struct Addresses {
        pub db: Addr<DatabaseActor>,
        pub broker: Addr<BrokerActor>,
        pub redis: Addr<RedisActor>,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct ChatHistoryRequest {
        pub chat_id: Uuid,
        pub page_index: Option<PageIndex>,
        pub page_size: usize,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct UserName {
        pub user_name: String,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct UserId {
        pub user_id: i64,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct UserInfoStripped {
        pub id: i64,
        pub name: String,
    }

    impl From<UserInfo> for UserInfoStripped {
        fn from(value: UserInfo) -> Self {
            UserInfoStripped {
                id: value.id,
                name: value.name,
            }
        }
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct ChatId {
        pub chat_id: Uuid,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct UserInvitation {
        pub guest_id: i64,
        pub chat_id: Uuid,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct PrivateChatCreationInfo {
        pub guest_user: i64,
        pub new_chat_name: String,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct GroupChatCreationInfo {
        pub guest_users: String,
        pub new_chat_name: String,
    }
}

#[post("/new-private")]
async fn create_new_private_chat(
    user_id: web::ReqData<i64>,
    new_chat: web::Query<data_types::PrivateChatCreationInfo>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let creator_id = user_id.into_inner();
    let new_chat = new_chat.into_inner();
    let new_chat_info = data
        .db
        .send(database_actor::messages::CreateNewPrivateChat {
            creator_id,
            chat_name: new_chat.new_chat_name,
            invited_user_id: new_chat.guest_user,
        })
        .await
        .expect("Sending message to database actor -> Failed");
    match new_chat_info {
        Ok(info) => HttpResponse::Ok()
            .body(serde_json::to_string(&info).expect("Cannot convert chat info to string")),
        Err(DBError::LogicError(e)) => HttpResponse::Conflict().body(e.to_string()),
        Err(DBError::QueryError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
        Err(DBError::OtherError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

/// Создать новый групповой чат
///
/// Создает чат, приглашает в него пользователей и возвращает данные о чате
#[post("/new-group")]
async fn create_new_group_chat(
    user_id: web::ReqData<i64>,
    data: web::Data<data_types::Addresses>,
    new_chat: web::Query<data_types::GroupChatCreationInfo>,
) -> impl Responder {
    let new_chat = new_chat.into_inner();
    let creator_id = user_id.into_inner();
    let chat_name = new_chat.new_chat_name;
    let invited_users_id = if let Ok(v) = serde_json::from_str::<Vec<i64>>(&new_chat.guest_users) {
        v
    } else {
        return HttpResponse::BadRequest().body("Malformed json format for guest user ids");
    };
    let new_chat_info = data
        .db
        .send(database_actor::messages::CreateNewGroupChat {
            creator_id,
            chat_name,
            invited_users_id,
        })
        .await
        .expect("Sending message to database actor -> Failed");
    match new_chat_info {
        Ok(info) => HttpResponse::Ok()
            .body(serde_json::to_string(&info).expect("Cannot convert chat info to string")),
        Err(DBError::LogicError(e)) => HttpResponse::Conflict().body(e.to_string()),
        Err(DBError::QueryError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
        Err(DBError::OtherError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

/// Пригласить пользователя в чат
///
/// Если приглашающий не состоит в данном чате или приглашенного пользователя в принципе не
/// существует, то возвращается Forbidden
///
/// /api/chat/invite-user?guest_id={id пользователя}&chat_id={id чата}
#[put("/new-user")]
async fn add_user_to_chat(
    user_id: web::ReqData<i64>,
    invite_info: web::Query<data_types::UserInvitation>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let user_id = user_id.into_inner();
    let invite_info = invite_info.into_inner();
    let result = data
        .db
        .send(database_actor::messages::InviteUserToChat {
            user_id,
            guest_user_id: invite_info.guest_id,
            chat_id: invite_info.chat_id,
        })
        .await
        .expect("Sending message to Database actor -> Failed");
    match result {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(DBError::LogicError(e)) => HttpResponse::Forbidden().body(e.to_string()),
        Err(DBError::QueryError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
        Err(DBError::OtherError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

/// Выйти из чата
///
/// Берет id пользователя из токена, id чата из аргументов и выходит из чата
/// Если чат при выходе становится пустым, то он удаляется.
///
/// Если пользователь не состоял в чате, или чата не существует, то выдаем Conflict
///
/// /api/chat/exit?chat_id={id чата}
#[put("/exit")]
async fn exit_chat(
    user_id: web::ReqData<i64>,
    chat_id: web::Query<data_types::ChatId>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let user_id = user_id.into_inner();
    let chat_id = chat_id.chat_id;
    let result = data
        .db
        .send(database_actor::messages::ExitChat { user_id, chat_id })
        .await
        .expect("Sending message to Database actor -> Failed");
    match result {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(DBError::LogicError(e)) => HttpResponse::Conflict().body(e.to_string()),
        Err(DBError::QueryError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
        Err(DBError::OtherError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

/// Получить информацию о чате
///
/// Берем id пользователя из токена и id чата из аргумента, возвращаем инфу о чате
/// Если пользователь не состоит в чате или чата не существует, то возвращаем Forbidden
///
/// /api/chat/info?chat_id={id чата} = {id: Uuid, name: String, users: [i64], chat_type: String}
#[get("/info")]
async fn get_chat_info(
    chat_id: web::Query<data_types::ChatId>,
    data: web::Data<data_types::Addresses>,
    user_id: web::ReqData<i64>,
) -> impl Responder {
    let user_id = user_id.into_inner();
    let chat_id = chat_id.chat_id;
    let chat_info = data
        .db
        .send(database_actor::messages::GetChatInfo { user_id, chat_id })
        .await
        .expect("Sending message to Database actor -> Failed");
    let chat_info = match chat_info {
        Ok(info) => info,
        Err(DBError::LogicError(e)) => return HttpResponse::Forbidden().body(e.to_string()),
        Err(DBError::QueryError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
        Err(DBError::OtherError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
    };
    HttpResponse::Ok().body(serde_json::to_string(&chat_info).unwrap())
}

/// Получить информацию о пользователе
///
/// Получаем информацию о любом пользователе, указав его id через аргумент user_id
/// Токен не используется, но обязателен для доступа к апи
///
/// Если пользователя не существует, то возвращаем NotFound
///
/// /api/user/info?user_id={id пользователя} = {id: i64, name: String}
#[get("/info")]
async fn get_user_info(
    user_id: web::Query<data_types::UserId>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let user_id = user_id.user_id;
    let user_info = data
        .db
        .send(database_actor::messages::GetUserInfo { user_id })
        .await
        .expect("Sending message to Database actor -> Failed");
    let user_info: data_types::UserInfoStripped = match user_info {
        Ok(info) => info.into(),
        Err(DBError::LogicError(e)) => return HttpResponse::NotFound().body(e.to_string()),
        Err(DBError::OtherError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
        Err(DBError::QueryError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
    };
    return HttpResponse::Ok()
        .body(serde_json::to_string(&user_info).expect("Failed converting user info to json"));
}

/// Получить чаты текущего пользователя
///
/// Берет id пользователя из токена и возвращает список UUID чатов
///
/// Если не вышло, значит возвращаем Unauthorized
///
/// /api/user/chats = {[UUID]}
#[get("/chats")]
async fn get_user_chats(
    user_id: ReqData<i64>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let chats = data
        .db
        .send(database_actor::messages::GetUserChats {
            user_id: user_id.into_inner(),
        })
        .await
        .expect("Sending message to Database actor -> Failed");
    let chats = match chats {
        Ok(c) => c,
        Err(DBError::LogicError(e)) => return HttpResponse::Unauthorized().body(e.to_string()),
        Err(DBError::OtherError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
        Err(DBError::QueryError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
    };
    HttpResponse::Ok()
        .body(serde_json::to_string(&chats).expect("Failed converting user chats to json"))
}

/// Авторизация пользователя в сервисе чата
///
/// Берет id пользователя из токена и либо создает новый аккаунт в чате,
/// либо ничего не делает, после чего возвращает данные о пользователе.
/// Данный запрос может использоваться к
///
/// Этот запрос необходимо делать каждый раз, когда пользователь только подключается к сервису
/// чата, ибо может выйти так, что аккаунта пользователя в чате не сущетвует, из-за чего многие
/// запросы будут выдавать ошибку Unauthorized
///
/// /api/user/authorize?user_name={имя пользователя} = {id: i64, name: String, chats: [UUID]}
#[post("/authorization")]
async fn authorize_user(
    user_id: ReqData<i64>,
    data: web::Data<data_types::Addresses>,
    user_name: web::Query<data_types::UserName>,
) -> impl Responder {
    let user_name = user_name.into_inner().user_name;
    let user_id = user_id.into_inner();
    let user_info = data
        .db
        .send(database_actor::messages::GetUserInfo { user_id })
        .await
        .expect("Sending message to Database actor -> Failed");
    let user_info = match user_info {
        Ok(info) => info,
        Err(DBError::LogicError(_)) => {
            let new_info = data
                .db
                .send(database_actor::messages::CreateNewUser { user_id, user_name })
                .await
                .expect("Sending message to Database actor -> Failed")
                .expect("User creation failed, bruh moment");
            new_info
        }
        Err(DBError::QueryError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
        Err(DBError::OtherError(e)) => {
            return HttpResponse::InternalServerError().body(e.to_string())
        }
    };
    HttpResponse::Ok().body(serde_json::to_string(&user_info).expect("Cannot serialize user info"))
}

/// Получить предудыщуие сообщения из чата с пагинацией
/// page_index может не присутствовать, при первом запросе, однако, он обязан быть при последующих
/// Индекс можно получить из первого запроса
/// /api/chat/history?chat_id={id_чата}&page_index={индекс}&page_size={размер_страницы}
/// = {[[сообщения], индекс]}
#[get("/history")]
async fn get_chat_history(
    user_id: ReqData<i64>,
    req: web::Query<data_types::ChatHistoryRequest>,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let user_id = user_id.into_inner();
    let req_info = req.into_inner();
    let chat_id = req_info.chat_id;
    let page_index = req_info.page_index;
    let page_size = req_info.page_size;
    let chat_history = data
        .db
        .send(database_actor::messages::GetChatHistory {
            user_id,
            chat_id,
            page_size,
            page_index,
        })
        .await
        .expect("Sending message to Database actor -> Failed");
    match chat_history {
        Ok(v) => HttpResponse::Ok().body(serde_json::to_string(&v).unwrap()),
        Err(DBError::LogicError(e)) => HttpResponse::Forbidden().body(e.to_string()),
        Err(DBError::QueryError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
        Err(DBError::OtherError(e)) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

#[get("/ws")]
async fn websocket_startup(
    req: HttpRequest,
    user_id: ReqData<i64>,
    stream: web::Payload,
    data: web::Data<data_types::Addresses>,
) -> impl Responder {
    let user_id = user_id.into_inner();
    let user_info = data
        .db
        .send(database_actor::messages::GetUserInfo { user_id })
        .await
        .expect("Sending message to Database actor -> Failed");
    match user_info {
        Ok(_) => {}
        Err(DBError::LogicError(e)) => return Ok(HttpResponse::Unauthorized().body(e.to_string())),
        Err(DBError::OtherError(e)) => {
            return Ok(HttpResponse::InternalServerError().body(e.to_string()))
        }
        Err(DBError::QueryError(e)) => {
            return Ok(HttpResponse::InternalServerError().body(e.to_string()))
        }
    }
    let new_websocket = WebsocketActor::new(
        data.broker.clone(),
        data.redis.clone(),
        data.db.clone(),
        user_id,
    );
    let resp = ws::start(new_websocket, &req, stream);
    resp
}
