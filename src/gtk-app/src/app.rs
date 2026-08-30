use crate::mainwindow::MainWindow;

pub struct Application;

impl Application {
    pub fn init_resources() {
        gtk_fm_ui::init_resources();
        gtk_sysinfo_ui::init_resources();
        gtk_terminal_ui::init_resources();
        gtk_registry_ui::init_resources();
        gtk_process_ui::init_resources();
    }

    pub fn run(app: &adw::Application, config: client_config::AppConfig) {
        ic_utils::app::init_exe_path();

        let mut needs_save = false;

        let device_id = if let Some(id) = config.get::<String>("app.device_id") {
            id
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            config.set("app.device_id", &id);
            needs_save = true;
            id
        };

        if config.get::<Vec<String>>("ui.favorites").is_none() {
            config.set(
                "ui.favorites",
                vec!["local_fs:/".to_string(), "local_fs:~".to_string()],
            );
            needs_save = true;
        }

        if needs_save {
            config.save();
        }

        crate::logging::apply(&config);
        ic_logging::info!(
            "Ice Commander {} ({}) starting",
            common::version::APP_VERSION,
            common::version::BUILD_TYPE
        );

        virtualfs::set_connect_timeout_secs(config.get::<u64>("net.connect_timeout_secs").unwrap_or(20));
        virtualfs::set_request_timeout_secs(config.get::<u64>("net.request_timeout_secs").unwrap_or(20));

        let my_info = ic_model::DeviceInfo {
            id: device_id,
            name: crate::device_name::current(&config),
            os: std::env::consts::OS.to_string(),
            version: common::version::APP_VERSION.to_string(),
            app_type: "desktop".to_string(),
            build_type: common::version::BUILD_TYPE.to_string(),
        };

        print_startup_banner(&my_info);

        let args: Vec<String> = std::env::args().collect();
        let headless = args.iter().any(|a| a == "--headless");
        let webui = args.iter().any(|a| a == "--webui");
        let port: u16 = args.iter()
            .skip_while(|a| *a != "--port")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(7878);
        let host: String = args.iter()
            .skip_while(|a| *a != "--host")
            .nth(1)
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let network_warning: Option<(String, u16)>;
        let api_tx = if headless {
            let is_network = host != "127.0.0.1" && host != "localhost";
            if is_network {
                eprintln!("[API] WARNING: binding to {host} — REST API accessible from other machines on the network!");
            }
            network_warning = if is_network { Some((host.clone(), port)) } else { None };
            let (tx, rx) = tokio::sync::mpsc::channel::<crate::api::ApiCmd>(64);
            let ws_sessions = crate::api::WsSessions::default();
            crate::api::init_notifier(ws_sessions.clone());
            let term_out_left = tokio::sync::broadcast::channel::<Vec<u8>>(1024).0;
            let term_out_right = tokio::sync::broadcast::channel::<Vec<u8>>(1024).0;
            crate::api::start_api_server(
                port, webui, host,
                tx.clone(), ws_sessions.clone(),
                term_out_left.clone(), term_out_right.clone(),
                include_bytes!("../assets/webui/bundle.js").to_vec(),
                include_bytes!("../assets/webui/style.css").to_vec(),
            );
            Some((tx, rx, ws_sessions, term_out_left, term_out_right))
        } else {
            network_warning = None;
            None
        };

        MainWindow::create(app, my_info, config.clone(), api_tx, network_warning);
    }
}

fn print_startup_banner(my_info: &ic_model::DeviceInfo) {
    println!("====================================================");
    println!("🚀 IceCommander - Desktop Node Starting Up");
    println!("   Version   : v{}", my_info.version);
    println!("   OS        : {}", my_info.os);
    println!("   App Type  : {}", my_info.app_type);
    println!("   Build Type: {}", my_info.build_type);
    println!("   Node ID   : {}", my_info.id);
    println!("====================================================");
}
