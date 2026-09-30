use fm_core::rpc::FileSystemRpc;
use std::rc::Rc;

pub struct EndpointPlan {
    kind: Endpoint,
    display_prefix: String,
    rel_prefix: String,
}

struct Gone;

#[async_trait::async_trait(?Send)]
impl FileSystemRpc for Gone {}

enum Endpoint {
    Local { config: client_config::AppConfig },
    Mounted { kind: String, settings: String },
}

impl EndpointPlan {
    pub fn of(provider: &Rc<dyn FileSystemRpc>) -> Option<Self> {
        let any = provider.as_any()?;
        let (inner, display_prefix, rel_prefix) =
            match any.downcast_ref::<panel_router::RoutingProvider>() {
                Some(rp) => (
                    rp.inner(),
                    rp.display_prefix().to_string(),
                    rp.rel_prefix().to_string(),
                ),
                None => (provider.clone(), String::new(), "/".to_string()),
            };
        let kind = Self::classify(&inner)?;
        Some(Self {
            kind,
            display_prefix,
            rel_prefix,
        })
    }

    fn classify(provider: &Rc<dyn FileSystemRpc>) -> Option<Endpoint> {
        let any = provider.as_any()?;
        if let Some(p) = any.downcast_ref::<localfs::local_rpc::LocalFileSystemRpc>() {
            return Some(Endpoint::Local {
                config: p.config.clone(),
            });
        }
        if let Some(p) = any.downcast_ref::<fm_core::plugin_fs::PluginFsRpc>() {
            let (kind, settings) = p.connection_origin()?;
            return Some(Endpoint::Mounted { kind, settings });
        }
        None
    }

    pub fn into_factory(self) -> transfer_core::ProviderFactory {
        Box::new(move || {
            let base: Rc<dyn FileSystemRpc> = match self.kind {
                Endpoint::Local { config } => {
                    Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(config))
                }
                Endpoint::Mounted { kind, settings } => {
                    let settings: std::collections::BTreeMap<String, String> =
                        serde_json::from_str(&settings).unwrap_or_default();
                    ic_plugin_host::mount_connection(&kind, &settings)
                        .unwrap_or_else(|| Rc::new(Gone))
                }
            };
            Rc::new(panel_router::RoutingProvider::from_parts(
                base,
                self.display_prefix,
                self.rel_prefix,
            ))
        })
    }
}
