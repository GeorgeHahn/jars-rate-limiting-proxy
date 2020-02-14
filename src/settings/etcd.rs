use crate::Res;
use async_trait::async_trait;
use futures::stream::Stream;
use futures_util::StreamExt;

#[async_trait]
pub trait ConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = String>>>;
}

pub struct EtcdConfigProvider {
    client: etcd_rs::Client,
}

#[async_trait]
impl ConfigProvider for EtcdConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = String>>> {
        use etcd_rs::*;

        // TODO: Do a Get before the streaming watch

        // The etcd-rs api leaves something to be desired. This uses an experimental refactoring
        // that is not (currently) upstreamed.

        Ok(Box::new(
            self.client
                .watch(KeyRange::key("base_path"))
                .filter_map(|val| async move {
                    // TODO
                    Some("".to_owned())
                }),
        ))
    }
}

impl EtcdConfigProvider {
    pub fn connect() -> Self {
        todo!("connect")
        // let client = Client::connect(ClientConfig {
        //     endpoints: vec!["http://127.0.0.1:2379".to_owned()],
        //     auth: None,
        // })
        // .await?;
    }
}
