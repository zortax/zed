//! Carrying Zed's HTTP client through gpui.
//!
//! gpui used to hold Zed's client itself: `cx.set_http_client` took an `Arc<dyn HttpClient>` and
//! every consumer read it back with `cx.http_client()`. gpui-ce narrowed its own `HttpClient` to a
//! single `get` so that gpui can be published without Zed's networking stack behind it, and a
//! one-method trait cannot round-trip a client that Zed asks for `send`, `proxy` and a user agent.
//!
//! So the carrier moves here. Zed's client lives in a gpui global, and gpui is handed an adapter
//! over the same client rather than a second one — which matters, because gpui fetches remote
//! images through it and those requests must go through the user's proxy like every other.

use std::sync::Arc;

use futures::AsyncReadExt as _;
use futures::future::BoxFuture;
use gpui::{App, Global};

use crate::{AsyncBody, HttpClient};

/// Zed's HTTP client, for the consumers gpui's narrowed trait cannot serve.
#[derive(Clone)]
struct GlobalHttpClient(Arc<dyn HttpClient>);

impl Global for GlobalHttpClient {}

/// Installs `client` as the one Zed and gpui both use.
///
/// Both, deliberately: gpui gets an adapter over the same client rather than its own, so there is
/// a single place where Zed's proxy and user agent are configured.
pub fn set_http_client(cx: &mut App, client: Arc<dyn HttpClient>) {
    cx.set_global(GlobalHttpClient(client.clone()));
    cx.set_http_client(Arc::new(GpuiHttpClient(client)));
}

/// Zed's HTTP client.
///
/// Panics if [`set_http_client`] has not run, which is the same contract `cx.http_client()` had:
/// a Zed that reached the network before configuring how is a bug, not a fallback.
pub fn http_client(cx: &App) -> Arc<dyn HttpClient> {
    cx.global::<GlobalHttpClient>().0.clone()
}

/// Zed's HTTP client, or `None` when one was never installed.
///
/// For the paths that legitimately run without networking configured, chiefly tests.
pub fn try_http_client(cx: &App) -> Option<Arc<dyn HttpClient>> {
    cx.try_global::<GlobalHttpClient>()
        .map(|global| global.0.clone())
}

/// Zed's client, in the shape gpui asks for.
struct GpuiHttpClient(Arc<dyn HttpClient>);

impl gpui::http_client::HttpClient for GpuiHttpClient {
    fn get(
        &self,
        url: &str,
        follow_redirects: bool,
    ) -> BoxFuture<'static, anyhow::Result<gpui::http_client::HttpResponse>> {
        let response = self.0.get(url, AsyncBody::empty(), follow_redirects);
        Box::pin(async move {
            let mut response = response.await?;
            // gpui wants the whole body, because what it does with one is decode an image.
            let mut body = Vec::new();
            response.body_mut().read_to_end(&mut body).await?;
            Ok(gpui::http_client::HttpResponse {
                status: response.status(),
                body,
            })
        })
    }
}
