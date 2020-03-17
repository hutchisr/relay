use crate::state::State;
use activitystreams::primitives::XsdAnyUri;
use actix::prelude::*;
use bb8_postgres::tokio_postgres::{tls::NoTls, AsyncMessage, Client, Config, Notification};
use futures::{
    future::ready,
    stream::{poll_fn, StreamExt},
};
use log::{debug, error, info};
use tokio::sync::mpsc;

#[derive(Message)]
#[rtype(result = "()")]
pub struct Notify(Notification);

pub struct NotifyHandler {
    client: Option<Client>,
    state: State,
    config: Config,
}

impl NotifyHandler {
    fn new(state: State, config: Config) -> Self {
        NotifyHandler {
            state,
            config,
            client: None,
        }
    }

    pub fn start_handler(state: State, config: Config) -> Addr<Self> {
        Supervisor::start(|_| Self::new(state, config))
    }
}

impl Actor for NotifyHandler {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let config = self.config.clone();

        let fut = async move {
            let (client, mut conn) = match config.connect(NoTls).await {
                Ok((client, conn)) => (client, conn),
                Err(e) => {
                    error!("Error establishing DB Connection, {}", e);
                    return Err(());
                }
            };

            let mut stream = poll_fn(move |cx| conn.poll_message(cx)).filter_map(|m| match m {
                Ok(AsyncMessage::Notification(n)) => {
                    debug!("Handling Notification, {:?}", n);
                    ready(Some(Notify(n)))
                }
                Ok(AsyncMessage::Notice(e)) => {
                    debug!("Handling Notice, {:?}", e);
                    ready(None)
                }
                Err(e) => {
                    debug!("Handling Error, {:?}", e);
                    ready(None)
                }
                _ => {
                    debug!("Handling rest");
                    ready(None)
                }
            });

            let (mut tx, rx) = mpsc::channel(256);

            Arbiter::spawn(async move {
                debug!("Spawned stream handler");
                while let Some(n) = stream.next().await {
                    match tx.send(n).await {
                        Err(e) => error!("Error forwarding notification, {}", e),
                        _ => (),
                    };
                }
                debug!("Stream handler ended");
            });

            Ok((client, rx))
        };

        let fut = fut.into_actor(self).map(|res, actor, ctx| match res {
            Ok((client, stream)) => {
                Self::add_stream(stream, ctx);
                let f = async move {
                    match crate::db::listen(&client).await {
                        Err(e) => {
                            error!("Error listening, {}", e);
                            Err(())
                        }
                        Ok(_) => Ok(client),
                    }
                };

                ctx.wait(f.into_actor(actor).map(|res, actor, ctx| match res {
                    Ok(client) => {
                        actor.client = Some(client);
                    }
                    Err(_) => {
                        ctx.stop();
                    }
                }));
            }
            Err(_) => {
                ctx.stop();
            }
        });

        ctx.wait(fut);
        info!("Listener starting");
    }
}

impl StreamHandler<Notify> for NotifyHandler {
    fn handle(&mut self, Notify(notif): Notify, ctx: &mut Self::Context) {
        let state = self.state.clone();

        info!("Handling notification in {}", notif.channel());
        let fut = async move {
            match notif.channel() {
                "new_blocks" => {
                    debug!("Caching block of {}", notif.payload());
                    state.cache_block(notif.payload().to_owned()).await;
                }
                "new_whitelists" => {
                    debug!("Caching whitelist of {}", notif.payload());
                    state.cache_whitelist(notif.payload().to_owned()).await;
                }
                "new_listeners" => {
                    if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                        debug!("Caching listener {}", uri);
                        state.cache_listener(uri).await;
                    }
                }
                "rm_blocks" => {
                    debug!("Busting block cache for {}", notif.payload());
                    state.bust_block(notif.payload()).await;
                }
                "rm_whitelists" => {
                    debug!("Busting whitelist cache for {}", notif.payload());
                    state.bust_whitelist(notif.payload()).await;
                }
                "rm_listeners" => {
                    if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                        debug!("Busting listener cache for {}", uri);
                        state.bust_listener(&uri).await;
                    }
                }
                _ => (),
            }
        };

        ctx.spawn(fut.into_actor(self));
    }
}

impl Supervised for NotifyHandler {}