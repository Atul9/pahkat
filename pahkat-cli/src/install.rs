use std::path::Path;
use std::sync::Arc;

use futures::stream::StreamExt;

use pahkat_client::{
    transaction::{PackageAction, PackageTransaction},
    package_store::InstallTarget,
    PackageStore,
    PackageKey,
};
use crate::Platform;

pub(crate) async fn install<'a>(
    store: Arc<dyn PackageStore>,
    packages: &'a Vec<String>,
    target: InstallTarget,
    args: &'a crate::Args,
) -> Result<(), anyhow::Error> {
    let keys: Vec<PackageKey> = packages
        .iter()
        .map(|id| {
            let mut key: PackageKey = store
                .find_package_by_id(id)
                .map(|x| x.0)
                .ok_or_else(|| anyhow::anyhow!("Could not find package for: `{}`", id))?;

            if let Some(platform) = args.platform() {
                key.query.platform = Some(platform.to_string());
            }

            Ok(key)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // for key in keys.iter() {
    //     // let pb = indicatif::ProgressBar::new(0);
    //     // pb.set_style(indicatif::ProgressStyle::default_bar()
    //     //     .template("{spinner:.green} {prefix} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
    //     //     .progress_chars("=>-"));
    //     // pb.set_prefix(&key.id);

    //     let progress = Box::new(move |cur, max| {
    //         // pb.set_length(max);
    //         // pb.set_position(cur);

    //         // if cur >= max {
    //         //     pb.finish_and_clear();
    //         // }
    //         true
    //     });

    //     let _ = store.download(&key, progress)?;
    // }

    let transaction = PackageTransaction::new(
        store,
        keys.iter()
            .map(|x| PackageAction::install(x.clone(), target.clone()))
            .collect(),
    )?;

    let (canceler, mut tx) = transaction.process();

    if let Some(event) = tx.next().await {
        // TODO;
    }
    // transaction
    //     .process(|key, event| {
    //         println!("{:?}: {:?}", &key, &event);
    //         true
    //     })
    //     .join()
    //     .unwrap()?;
    Ok(())
}
