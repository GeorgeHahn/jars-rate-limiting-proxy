use crate::settings::DynamicSettings;
use crate::Res;
use etcd_rs::{Client, ClientConfig};
use futures::{future, stream::Stream};
use futures_util::StreamExt;
use tokio::task;

pub struct EtcdConfigProvider {
    client: Client,
}

impl EtcdConfigProvider {
    pub async fn connect(etcd_uris: Vec<String>) -> Res<Self> {
        // Note: this doesn't actually connect to the cluster (that's done in top-level api calls on
        // `client`)
        let client = Client::connect(ClientConfig {
            endpoints: etcd_uris,
            auth: None,
        })
        .await?;
        Ok(EtcdConfigProvider { client })
    }

    pub async fn get_and_stream(
        self,
        etcd_key_prefix: String,
    ) -> Res<Box<dyn Stream<Item = DynamicSettings> + Unpin + Send>> {
        use etcd_rs::*;

        // critical TODO: send a get request before the streaming watch

        // The etcd-rs api leaves something to be desired. This uses an experimental refactoring
        // that is not (currently) upstreamed.

        // Used as a workaround to unroll a Stream<Vec<Item>> -> Stream<Item> (details below)
        let (tx, rx) = tokio::sync::mpsc::channel(1);

        let watch = self
            .client
            .watch(KeyRange::prefix(etcd_key_prefix.clone()))
            .await
            .filter_map(|val| {
                // Drop errors from the stream

                future::ready(match val {
                    Ok(val) => Some(val),
                    Err(e) => {
                        println!("etcd watch error: {}", e);
                        None
                    }
                })
            })
            .map(move |mut watch_resp| {
                // Get all events from incoming watch response
                let events = watch_resp.take_events();
                let etcd_key_prefix = etcd_key_prefix.to_owned();

                // Map incoming events to a vector of `DynamicSettings` values
                events.into_iter().filter_map(move |mut event| {
                    let kv = match event.take_kvs() {
                        Some(kv) => kv,
                        None => return None,
                    };

                    let key = std::str::from_utf8(kv.key());
                    let val = std::str::from_utf8(kv.value());

                    if key.is_err() || val.is_err() {
                        // todo: log(trace) this
                        return None;
                    }
                    let (key, val) = (key.unwrap(), val.unwrap());
                    println!("etcd incoming: {:?}, {:?}", key, val);

                    // Remove customizable prefix from key name
                    let key = key.replace(&etcd_key_prefix, "");

                    // Convert string key & value into a typed `Option<DynamicSettings>`
                    // todo: log(trace) parse errors
                    // todo: log(trace) unknown keys
                    match key.as_ref() {
                        "base_path" => {
                            return val.parse().ok().map(|v| DynamicSettings::BasePath(v))
                        }
                        "error_path" => {
                            return val.parse().ok().map(|v| DynamicSettings::ErrorPath(v))
                        }
                        _ => None,
                    }
                })
            })
            .for_each(move |vals| {
                // Expand incoming vectors of `DynamicSettings` values to multiple stream items.
                //
                // It feels like this functionality should be included with StreamExt, but I don't
                // see an appropriate function. Need to ask on the Rust Discord and refactor or
                // contribute appropriately.

                for val in vals {
                    println!("{:?}", val);
                    let mut tx = tx.clone();
                    // Really don't like that each value needs to be sent as a separate task, but
                    // there's a nasty lifetime issue on `tx` otherwise. Probably surmountable, but
                    // I'd rather work to remove this entire workaround.
                    task::spawn(async move {
                        // suppress result warning; this should only fail if the mpsc receiver is
                        // dropped (which we can't do anything about)
                        let _ = tx.send(val).await;
                    });
                }
                future::ready(())
            });

        // This is suboptimal - the watch will continue producing items even once the rx stream is
        // dropped. The ugly stream unroll above should be refactored to eliminate this possibility.
        task::spawn(watch);

        Ok(Box::new(rx))
    }
}
