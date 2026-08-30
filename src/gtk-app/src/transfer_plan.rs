
use fm_core::rpc::FileSystemRpc;
use std::rc::Rc;

pub struct EndpointPlan {
    kind: Endpoint,
    display_prefix: String,
    rel_prefix: String,
}

enum Endpoint {
    Local {
        config: client_config::AppConfig,
    },
    WebDav {
        name: String,
        url: String,
        user: Option<String>,
        pass: Option<String>,
        remote_path: Option<String>,
    },
    Sftp(Box<SftpParams>),
    Ftp {
        name: String,
        host: String,
        port: u16,
        user: String,
        pass: String,
    },
}

struct SftpParams {
    name: String,
    host: String,
    port: u16,
    user: String,
    pass: Option<String>,
    auth_type: String,
    key_path: Option<String>,
    passphrase: Option<String>,
    use_tunnel: Option<bool>,
    tunnel_host: Option<String>,
    tunnel_port: Option<u16>,
    tunnel_user: Option<String>,
    tunnel_auth_type: Option<String>,
    tunnel_pass: Option<String>,
    tunnel_key_path: Option<String>,
    tunnel_passphrase: Option<String>,
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
        Some(Self { kind, display_prefix, rel_prefix })
    }

    fn classify(provider: &Rc<dyn FileSystemRpc>) -> Option<Endpoint> {
        let any = provider.as_any()?;
        if let Some(p) = any.downcast_ref::<virtualfs::local_rpc::LocalFileSystemRpc>() {
            return Some(Endpoint::Local { config: p.config.clone() });
        }
        if let Some(p) = any.downcast_ref::<virtualfs::webdav_rpc::LocalWebDavRpc>() {
            return Some(Endpoint::WebDav {
                name: p.name.clone(),
                url: p.url.clone(),
                user: p.user.clone(),
                pass: p.pass.clone(),
                remote_path: p.remote_path.clone(),
            });
        }
        if let Some(p) = any.downcast_ref::<virtualfs::sftp_rpc::LocalSftpRpc>() {
            return Some(Endpoint::Sftp(Box::new(SftpParams {
                name: p.name.clone(),
                host: p.host.clone(),
                port: p.port,
                user: p.user.clone(),
                pass: p.pass.clone(),
                auth_type: p.auth_type.clone(),
                key_path: p.key_path.clone(),
                passphrase: p.passphrase.clone(),
                use_tunnel: p.use_tunnel,
                tunnel_host: p.tunnel_host.clone(),
                tunnel_port: p.tunnel_port,
                tunnel_user: p.tunnel_user.clone(),
                tunnel_auth_type: p.tunnel_auth_type.clone(),
                tunnel_pass: p.tunnel_pass.clone(),
                tunnel_key_path: p.tunnel_key_path.clone(),
                tunnel_passphrase: p.tunnel_passphrase.clone(),
            })));
        }
        if let Some(p) = any.downcast_ref::<virtualfs::ftp_rpc::LocalFtpRpc>() {
            return Some(Endpoint::Ftp {
                name: p.name.clone(),
                host: p.host.clone(),
                port: p.port,
                user: p.user.clone(),
                pass: p.pass.clone(),
            });
        }
        None
    }

    pub fn into_factory(self) -> transfer_core::ProviderFactory {
        Box::new(move || {
            let base: Rc<dyn FileSystemRpc> = match self.kind {
                Endpoint::Local { config } => {
                    Rc::new(virtualfs::local_rpc::LocalFileSystemRpc::new(config))
                }
                Endpoint::WebDav { name, url, user, pass, remote_path } => {
                    Rc::new(virtualfs::webdav_rpc::LocalWebDavRpc {
                        name,
                        url,
                        user,
                        pass,
                        remote_path,
                    })
                }
                Endpoint::Sftp(p) => Rc::new(virtualfs::sftp_rpc::LocalSftpRpc {
                    name: p.name,
                    host: p.host,
                    port: p.port,
                    user: p.user,
                    pass: p.pass,
                    auth_type: p.auth_type,
                    key_path: p.key_path,
                    passphrase: p.passphrase,
                    use_tunnel: p.use_tunnel,
                    tunnel_host: p.tunnel_host,
                    tunnel_port: p.tunnel_port,
                    tunnel_user: p.tunnel_user,
                    tunnel_auth_type: p.tunnel_auth_type,
                    tunnel_pass: p.tunnel_pass,
                    tunnel_key_path: p.tunnel_key_path,
                    tunnel_passphrase: p.tunnel_passphrase,
                    sftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
                    tunnel: std::sync::Arc::new(std::sync::Mutex::new(None)),
                }),
                Endpoint::Ftp { name, host, port, user, pass } => {
                    Rc::new(virtualfs::ftp_rpc::LocalFtpRpc {
                        name,
                        host,
                        port,
                        user,
                        pass,
                        ftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
                    })
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
