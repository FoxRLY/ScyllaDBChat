use actix::Actor;
use actix_web::{
    self,
    middleware::Logger,
    web::{self},
    App, HttpServer,
};

use std::error::Error;

use chat::{
    actors::{
        broker_actor::BrokerActor,
        database_actor::{messages::InitDatabase, DatabaseActor},
        redis_actor::RedisActor,
    },
    handlers::{
        add_user_to_chat, authorize_user, create_new_group_chat, create_new_private_chat,
        data_types::Addresses, exit_chat, get_chat_history, get_chat_info, get_user_chats,
        get_user_info, websocket_startup,
    },
    middlewares::test_token_middleware::TestAuthMiddleware,
};

use log::info;
// Что вообще должен делать чат?
// - Принимать сообщения от пользователя +
// - Выдавать новые сообщения пользователю +
// - Создавать новые чаты по запросу +
// - Добавлять пользователя в чат по запросу участника +
// - Выходить из чата по запросу +
// - Выдавать информацию о чате, если пользователь - участник +
// - Выдавать информацию о пользователе, если ее просит пользователь +
// - Выдавать список чатов пользователя, если его просит пользователь +
//
// API чата:
// 1) /ws Вебсокет-соединение, через которое пользователь получает
// сообщения и отправляет новые
// 2) /api/create_new_chat
// 3) /api/add_user_to_chat
// 4) /api/exit_chat
// 5) /api/get_chat_info
// 6) /api/get_user_info
// 7) /api/get_user_chats

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("debug"));
    info!("Initializing service");
    let db = DatabaseActor::new("scylla-database".into(), 9042)
        .await
        .map_err(|e| e.to_string())?
        .start();
    info!("Connected to db");
    db.send(InitDatabase).await.unwrap().unwrap();
    info!("Initialized db");
    let broker = BrokerActor::new(db.clone()).await.start();
    let redis = RedisActor::new("redis-broker", 6379, broker.clone())
        .await
        .map_err(|e| e.to_string())?
        .start();
    info!("Connected to redis");
    let addrs = Addresses {
        db: db.clone(),
        broker: broker.clone(),
        redis: redis.clone(),
    };
    let data = web::Data::new(addrs);
    info!("Starting service");
    let _ = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .wrap(TestAuthMiddleware)
            .service(
                web::scope("/api")
                    .service(
                        web::scope("/user")
                            .service(authorize_user)
                            .service(get_user_info)
                            .service(get_user_chats),
                    )
                    .service(
                        web::scope("/chat")
                            .service(create_new_group_chat)
                            .service(create_new_private_chat)
                            .service(add_user_to_chat)
                            .service(exit_chat)
                            .service(get_chat_info)
                            .service(get_chat_history),
                    ),
            )
            .service(websocket_startup)
            .app_data(data.clone())
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await;
    Ok(())
}
