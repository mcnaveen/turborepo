use anyhow::Result;
use indexmap::IndexSet;
use mime::APPLICATION_JSON;
use turbo_tasks::primitives::{OptionStringVc, StringsVc};
use turbo_tasks_fs::File;
use turbopack_core::asset::AssetContentVc;
use turbopack_dev_server::source::{
    ContentSource, ContentSourceContent, ContentSourceData, ContentSourceResultVc, ContentSourceVc,
};
use turbopack_node::render::{
    node_api_source::NodeApiContentSourceVc, rendered_source::NodeRenderContentSourceVc,
};

/// A content source which creates the next.js `_devPagesManifest.json` and
/// `_devMiddlewareManifest.json` which are used for client side navigation.
#[turbo_tasks::value(shared)]
pub struct DevManifestContentSource {
    pub base_path: OptionStringVc,
    pub page_roots: Vec<ContentSourceVc>,
}

#[turbo_tasks::value_impl]
impl DevManifestContentSourceVc {
    #[turbo_tasks::function]
    async fn find_routes(self) -> Result<StringsVc> {
        let this = &*self.await?;
        let base_path = this.base_path.await?;
        let base_path_prefix = base_path.as_deref().unwrap_or("");

        let mut queue = this.page_roots.clone();
        let mut routes = IndexSet::new();

        while let Some(content_source) = queue.pop() {
            queue.extend(content_source.get_children().await?.iter());

            // TODO This shouldn't use casts but an public api instead
            if let Some(api_source) = NodeApiContentSourceVc::resolve_from(content_source).await? {
                routes.insert(format!(
                    "{}/{}",
                    base_path_prefix,
                    api_source.get_pathname().await?
                ));

                continue;
            }

            if let Some(page_source) =
                NodeRenderContentSourceVc::resolve_from(content_source).await?
            {
                routes.insert(format!(
                    "{}/{}",
                    base_path_prefix,
                    page_source.get_pathname().await?
                ));

                continue;
            }
        }

        routes.sort();

        Ok(StringsVc::cell(routes.into_iter().collect()))
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for DevManifestContentSource {
    #[turbo_tasks::function]
    async fn get(
        self_vc: DevManifestContentSourceVc,
        orig_path: &str,
        _data: turbo_tasks::Value<ContentSourceData>,
    ) -> Result<ContentSourceResultVc> {
        let this = self_vc.await?;

        let base_path = this.base_path.await?;

        let path = if let Some(base_path) = base_path.as_deref() {
            strip_base_path(orig_path, base_path)?
        } else {
            Some(orig_path)
        };

        let manifest_content = match path {
            Some("_next/static/development/_devPagesManifest.json") => {
                let pages = &*self_vc.find_routes().await?;

                serde_json::to_string(&serde_json::json!({
                    "pages": pages,
                }))?
            }
            Some("_next/static/development/_devMiddlewareManifest.json") => {
                // empty middleware manifest
                "[]".to_string()
            }
            _ => return Ok(ContentSourceResultVc::not_found()),
        };

        let file = File::from(manifest_content).with_content_type(APPLICATION_JSON);

        Ok(ContentSourceResultVc::exact(
            ContentSourceContent::Static(AssetContentVc::from(file).into()).cell(),
        ))
    }
}

/// Strips the base path from the given path. The base path must start with a
/// slash or be the empty string.
///
/// Returns `None` if the path does not start with the base path.
fn strip_base_path<'a, 'b>(path: &'a str, base_path: &'b str) -> Result<Option<&'a str>> {
    if base_path.is_empty() {
        return Ok(Some(path));
    }

    let base_path = base_path
        .strip_prefix('/')
        .ok_or_else(|| anyhow::anyhow!("base path must start with a slash, got {}", base_path))?;

    Ok(path
        .strip_prefix(base_path)
        .and_then(|path| path.strip_prefix('/')))
}
