use crate::{
    apub::{PublicKey, PublicKeyInner},
    config::{Config, UrlKind},
    data::State,
    error::MyError,
    routes::ok,
};
use activitystreams_ext::Ext1;
use activitystreams_new::{
    actor::{ApActor, Application, Endpoints},
    context,
    prelude::*,
    primitives::{XsdAnyUri, XsdString},
    security,
};
use actix_web::{web, Responder};
use rsa_pem::KeyExt;

pub async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<impl Responder, MyError> {
    let mut application = Ext1::new(
        ApActor::new(
            config.generate_url(UrlKind::Inbox).parse()?,
            Application::new(),
        ),
        PublicKey {
            public_key: PublicKeyInner {
                id: config.generate_url(UrlKind::MainKey).parse()?,
                owner: config.generate_url(UrlKind::Actor).parse()?,
                public_key_pem: state.public_key.to_pem_pkcs8()?,
            },
        },
    );

    application
        .set_id(config.generate_url(UrlKind::Actor).parse()?)
        .set_summary(XsdString::from("AodeRelay bot"))
        .set_name(XsdString::from("AodeRelay"))
        .set_url(config.generate_url(UrlKind::Actor).parse::<XsdAnyUri>()?)
        .set_many_contexts(vec![context(), security()])
        .set_preferred_username("relay".into())
        .set_outbox(config.generate_url(UrlKind::Outbox).parse()?)
        .set_followers(
            config
                .generate_url(UrlKind::Followers)
                .parse::<XsdAnyUri>()?,
        )
        .set_following(
            config
                .generate_url(UrlKind::Following)
                .parse::<XsdAnyUri>()?,
        )
        .set_endpoints(Endpoints {
            shared_inbox: Some(config.generate_url(UrlKind::Inbox).parse::<XsdAnyUri>()?),
            ..Default::default()
        });

    Ok(ok(application))
}
