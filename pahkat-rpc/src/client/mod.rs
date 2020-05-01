use futures::stream::{self, TryStreamExt};
use futures::Stream;
use parity_tokio_ipc::Endpoint as IpcEndpoint;
use std::convert::TryFrom;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use structopt::StructOpt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tonic::transport::server::Connected;
use tonic::transport::{Endpoint, Uri};
use tonic::{transport::Server, Request, Response, Status, Streaming};
use tower::service_fn;

use crate::pb;

#[derive(Debug, StructOpt)]
struct StatusCommand {
    package_id: String,
    target: String,
}

#[derive(Debug, StructOpt)]
struct RepoIndexesCommand {}

#[derive(Debug, StructOpt)]
struct ProcessTransactionCommand {
    // package-id::action[::target]
    actions: Vec<String>,
}

#[derive(Debug, StructOpt)]
struct StringsCommand {
    language: String,
}

#[derive(Debug, StructOpt)]
enum Command {
    Status(StatusCommand),
    RepoIndexes(RepoIndexesCommand),
    ProcessTransaction(ProcessTransactionCommand),
    Strings(StringsCommand),
}

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(subcommand)]
    command: Command,
}

type PahkatClient = pb::pahkat_client::PahkatClient<tonic::transport::channel::Channel>;
use once_cell::sync::Lazy;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

async fn new_client() -> anyhow::Result<PahkatClient> {
    let channel = Endpoint::try_from("file://tmp/pahkat")?
        .connect_with_connector(service_fn(|_: Uri| {
            let path = if cfg!(windows) {
                format!("//./pipe/pahkat")
            } else {
                format!("/tmp/pahkat")
            };

            IpcEndpoint::connect(path)
        }))
        .await?;

    let mut client = PahkatClient::new(channel);
    Ok(client)
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::from_args();
    let mut client = new_client().await?;

    match args.command {
        Command::Status(command) => {
            let request = Request::new(pb::StatusRequest {
                package_id: command.package_id,
                target: if command.target == "user" { 1 } else { 0 },
            });

            let response = client.status(request).await?;
            println!("{:#?}", response);
        }
        Command::RepoIndexes(_) => {
            let request = Request::new(pb::RepositoryIndexesRequest {});

            let response = client.repository_indexes(request).await?;
            println!("{:?}", response);
        }
        Command::ProcessTransaction(command) => {
            let actions = command
                .actions
                .into_iter()
                .map(|s| {
                    let mut s = s.split("::");
                    let id = s.next().unwrap().to_string();
                    let action = if s.next().unwrap_or_else(|| "install") == "install" {
                        0
                    } else {
                        1
                    };
                    let target = if s.next().unwrap_or_else(|| "system") != "user" {
                        0
                    } else {
                        1
                    };
                    pb::PackageAction { id, action, target }
                })
                .collect::<Vec<_>>();

            let req = stream::iter(vec![pb::TransactionRequest {
                value: Some(pb::transaction_request::Value::Transaction(
                    pb::transaction_request::Transaction { actions },
                )),
            }]);

            let request = Request::new(req);
            let stream = client.process_transaction(request).await?;

            let mut stream = stream.into_inner();

            while let Ok(Some(message)) = stream.message().await {
                println!("{:?}", message);
            }
        }
        Command::Strings(StringsCommand { language }) => {
            let request = Request::new(pb::StringsRequest { language });

            let response = client.strings(request).await?;
            println!("{:?}", response);
        } // Args::SetRepos
          // Args::Refresh
    }
    Ok(())
}

use cursed::{FromForeign, InputType, ReturnType, ToForeign};
use serde::Serialize;

pub struct JsonMarshaler;

impl InputType for JsonMarshaler {
    type Foreign = <cursed::StringMarshaler as InputType>::Foreign;
}

impl ReturnType for JsonMarshaler {
    type Foreign = cursed::Slice<u8>;

    fn foreign_default() -> Self::Foreign {
        cursed::Slice::default()
    }
}

impl<T> ToForeign<Result<T, Box<dyn Error>>, cursed::Slice<u8>> for JsonMarshaler
where
    T: Serialize,
{
    type Error = Box<dyn Error>;

    fn to_foreign(result: Result<T, Self::Error>) -> Result<cursed::Slice<u8>, Self::Error> {
        result.and_then(|input| {
            let json_str = serde_json::to_string(&input)?;
            Ok(cursed::StringMarshaler::to_foreign(json_str).unwrap())
        })
    }
}


pub struct JsonRefMarshaler<'a>(&'a std::marker::PhantomData<()>);

impl<'a> InputType for JsonRefMarshaler<'a> {
    type Foreign = <cursed::StrMarshaler<'a> as InputType>::Foreign;
}

impl<'a, T> FromForeign<cursed::Slice<u8>, T> for JsonRefMarshaler<'a>
where
    T: serde::de::DeserializeOwned,
{
    type Error = Box<dyn Error>;

    unsafe fn from_foreign(ptr: cursed::Slice<u8>) -> Result<T, Self::Error> {
        let json_str =
            <cursed::StrMarshaler<'a> as FromForeign<cursed::Slice<u8>, &'a str>>::from_foreign(
                ptr,
            )?;
        log::debug!("JSON: {}, type: {}", &json_str, std::any::type_name::<T>());

        let v: Result<T, _> = serde_json::from_str(&json_str);
        v.map_err(|e| {
            log::error!("Json error: {}", &e);
            log::debug!("{:?}", &e);
            Box::new(e) as _
        })
    }
}


#[cthulhu::invoke(return_marshaler = "cursed::ArcMarshaler::<RwLock<PahkatClient>>")]
pub extern "C" fn pahkat_rpc_new() -> Result<Arc<RwLock<PahkatClient>>, Box<dyn Error>> {
    let client = block_on(new_client())?;
    Ok(Arc::new(RwLock::new(client)))
}

#[no_mangle]
pub extern "C" fn pahkat_rpc_free(ptr: *const RwLock<PahkatClient>) {
    if ptr.is_null() {
        return;
    }
    unsafe { Arc::from_raw(ptr); }
}

#[no_mangle]
pub extern "C" fn pahkat_rpc_slice_free(slice: cursed::Slice<u8>) {
    unsafe { let _ = cursed::VecMarshaler::from_foreign(slice); }
}
#[cthulhu::invoke(return_marshaler = "JsonMarshaler")]
pub extern "C" fn pahkat_rpc_repo_indexes(
    #[marshal(cursed::ArcRefMarshaler::<RwLock<PahkatClient>>)] client: Arc<RwLock<PahkatClient>>,
) -> Result<pb::RepositoryIndexesResponse, Box<dyn Error>> {
    let request = Request::new(pb::RepositoryIndexesRequest {});

    let response = block_on(async move {
        let mut client = client.write().await;
        client.repository_indexes(request).await
    })?;

    let response: pb::RepositoryIndexesResponse = response.into_inner();
    Ok(response)
}

// #[cthulhu::invoke]
// pub extern "C" fn pahkat_rpc_status(
//     #[marshal(cursed::ArcRefMarshaler::<PahkatClient>)]
//     handle: &Arc<PahkatClient>
// ) {

// }

#[no_mangle]
extern "C" fn pahkat_rpc_cancel_callback() {
    let mut tx = CURRENT_CANCEL_TX.lock().unwrap();
    let cb = tx.borrow_mut().take();
    match cb {
        Some(tx) => tx.send(pb::TransactionRequest {
            value: Some(
                pb::transaction_request::Value::Cancel(pb::transaction_request::Cancel {})
            )
        }).unwrap(),
        None => {
            // No problem.
        }
    }
}

#[cthulhu::invoke(return_marshaler = "cursed::UnitMarshaler")]
pub extern "C" fn pahkat_rpc_process_transaction(
    #[marshal(cursed::ArcRefMarshaler::<RwLock<PahkatClient>>)] client: Arc<RwLock<PahkatClient>>,

    #[marshal(JsonRefMarshaler)]
    actions: Vec<pahkat_client::PackageAction>,

    callback: extern "C" fn(cursed::Slice<u8>),
// ) -> Result<extern "C" fn(), Box<dyn Error>> {
) -> Result<(), Box<dyn Error>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let mut global_tx = CURRENT_CANCEL_TX.lock().unwrap();
    *global_tx.borrow_mut() = Some(tx.clone());

    let request = Request::new(rx);

    spawn(async move {
        let mut client = client.write().await;

        let stream = client.process_transaction(request).await.unwrap();
    
        let mut stream = stream.into_inner();
    
        while let Ok(Some(message)) = stream.message().await {
            let cb_response: CallbackTransactionResponse = message.value.unwrap().into();
            let s = serde_json::to_string(&cb_response).unwrap();
            let bytes = s.as_bytes();

            unsafe {
                (callback)(cursed::Slice {
                    data: bytes.as_ptr() as *mut _,
                    len: bytes.len()
                });
            };
        }
    });

    tx.send(pb::TransactionRequest {
        value: Some(pb::transaction_request::Value::Transaction(
            pb::transaction_request::Transaction { actions: actions.into_iter().map(|x| x.into()).collect() },
        )),
    })?;

    // Ok(pahkat_rpc_cancel_callback)
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct CallbackTransactionResponse {
    #[serde(rename = "type")]
    ty: &'static str,

    #[serde(flatten)]
    value: pb::transaction_response::Value,
}

impl CallbackTransactionResponse {
    fn new(ty: &'static str, value: pb::transaction_response::Value) -> CallbackTransactionResponse {
        CallbackTransactionResponse {
            ty, value
        }
    }
}

impl From<pb::transaction_response::Value> for CallbackTransactionResponse {
    fn from(response: pb::transaction_response::Value) -> CallbackTransactionResponse {
        use pb::transaction_response::Value::*;

        match response {
            TransactionStarted(_) => Self::new("TransactionStarted", response),
            TransactionError(_) => Self::new("TransactionError", response),
            TransactionComplete(_) => Self::new("TransactionComplete", response),
            TransactionProgress(_) => Self::new("TransactionProgress", response),
            DownloadProgress(_) => Self::new("DownloadProgress", response),
            DownloadComplete(_) => Self::new("DownloadComplete", response),
            InstallStarted(_) => Self::new("InstallStarted", response),
            UninstallStarted(_) => Self::new("UninstallStarted", response),
        }
    }
}

static CURRENT_CANCEL_TX: Lazy<std::sync::Mutex<std::cell::RefCell<Option<
    tokio::sync::mpsc::UnboundedSender<pb::TransactionRequest>
>>>> = Lazy::new(|| std::sync::Mutex::new(std::cell::RefCell::new(None)));

static BASIC_RUNTIME: Lazy<std::sync::RwLock<tokio::runtime::Runtime>> = Lazy::new(|| {
    std::sync::RwLock::new(
        tokio::runtime::Builder::new()
            .threaded_scheduler()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime"),
    )
});

#[inline(always)]
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    BASIC_RUNTIME.write().unwrap().block_on(future)
}

#[inline(always)]
fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send
{
    BASIC_RUNTIME.write().unwrap().spawn(future)
}