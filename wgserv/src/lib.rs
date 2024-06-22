use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::sys::jstring;
use jni::JNIEnv;
use std::net::{IpAddr, SocketAddr};
use std::ptr::null_mut;
use tracing::{info, error};
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use udp_over_tcp::{self, tcp2udp};
use udp_over_tcp::tcp2udp::Tcp2UdpError;
use core::net::SocketAddrV4;
use core::net::Ipv4Addr;
use std::convert::Infallible;
use std::sync::Arc;

/*
JNIEXPORT jstring JNICALL Java_org_vi_1server_wgserver_Native_setConfig
JNIEXPORT jstring JNICALL Java_org_vi_1server_wgserver_Native_run
*/

struct App {
    config: Option<Config>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl App {
    pub fn new() -> App {
        App {
            config: None,
            shutdown: None,
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    debug: bool,

    pub private_key: String,
    pub peer_key: String,
    pub peer_endpoint: Option<SocketAddr>,
    pub keepalive_interval: Option<u16>,
    pub bind_ip_port: SocketAddr,

    pub dns_addr: Option<SocketAddr>,
    pub pingable: Option<IpAddr>,
    pub mtu: usize,
    pub tcp_buffer_size: usize,
    pub incoming_udp: Vec<PortForward>,
    pub incoming_tcp: Vec<PortForward>,

    pub transmit_queue_capacity: usize,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct PortForward {
    pub host: SocketAddr,
    pub src: Option<SocketAddr>,
    pub dst: SocketAddr,
}
impl From<PortForward> for libwgslirpy::router::PortForward {
    fn from(value: PortForward) -> Self {
        libwgslirpy::router::PortForward {
            host: value.host,
            src: value.src,
            dst: value.dst
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_vi_1server_wgserver_Native_create(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    Box::into_raw(Box::new(App::new())) as usize as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_vi_1server_wgserver_Native_destroy(
    _env: JNIEnv,
    _class: JClass,
    instance: jlong,
) {
    let mut app = unsafe { Box::from_raw(instance as usize as *mut App) };
    if let Some(shutdown) = app.shutdown.take() {
        let _ = shutdown.send(());
    }
    drop(app);
}

#[no_mangle]
pub extern "system" fn Java_org_vi_1server_wgserver_Native_setConfig(
    mut env: JNIEnv,
    _class: JClass,
    instance: jlong,
    input: JString,
) -> jstring {
    let input: String = env
        .get_string(&input)
        .expect("Couldn't get java string!")
        .into();

    match toml::from_str::<Config>(&input) {
        Ok(x) => {
            let mut failure: Option<&'static str> = None;

            if libwgslirpy::parsebase64_32(&x.peer_key).is_err() {
                failure = Some("Invalid peer_key")
            }
            if libwgslirpy::parsebase64_32(&x.private_key).is_err() {
                failure = Some("Invalid private_key")
            }

            if let Some(f) = failure {
                let output = env
                    .new_string(format!("{}", f))
                    .expect("Couldn't create java string!");
                output.into_raw()
            } else {
                let mut app = unsafe { Box::from_raw(instance as usize as *mut App) };
                app.config = Some(x);
                let _ = Box::into_raw(app);
                null_mut()
            }
        }
        Err(e) => {
            let output = env
                .new_string(format!("{}", e))
                .expect("Couldn't create java string!");
            output.into_raw()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_org_vi_1server_wgserver_Native_run(
    env: JNIEnv,
    _class: JClass,
    instance: jlong,
) -> jstring {

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Trace)
            .with_tag("WgServer"),
    );

    info!("Executing Java_org_vi_1server_wgserver_Native_run...");

    let mut app = unsafe { Box::from_raw(instance as usize as *mut App) };

    let Some(config) = app.config else {
        let _ = Box::into_raw(app);
        return env.new_string("setConfig should precede run").unwrap().into_raw()
    };

    let rt = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().unwrap();

    let (tx, rx_shutdown) = tokio::sync::oneshot::channel();
    app.shutdown = Some(tx);
    app.config = None;
    let _ = Box::into_raw(app);

    let notify_shutdown = Arc::new(tokio::sync::Notify::new());
    
    let notify_shutdown_clone = notify_shutdown.clone();
    rt.spawn(async move {
        let _ = rx_shutdown.await;
        notify_shutdown_clone.notify_waiters();
    });

    let _tracing = {
        let s = tracing_subscriber::registry();
        let a = tracing_android::layer("WgServer").unwrap();
        let lf: Option<_> = if !config.debug {
            Some(tracing_subscriber::filter::LevelFilter::INFO)
        } else {
            None
        };
        tracing::subscriber::set_default(s.with(a).with(lf))
    };

    let bind_ip_port = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, config.bind_ip_port.port()));

    let router_config = libwgslirpy::router::Opts {
        dns_addr: config.dns_addr,
        pingable: config.pingable,
        mtu: config.mtu,
        tcp_buffer_size: config.tcp_buffer_size,
        incoming_udp: config.incoming_udp.into_iter().map(|x|x.into()).collect(),
        incoming_tcp: config.incoming_tcp.into_iter().map(|x|x.into()).collect(),
    };
    let wg_config = libwgslirpy::wg::Opts {
        private_key: libwgslirpy::parsebase64_32(&config.private_key).unwrap().into(),
        peer_key: libwgslirpy::parsebase64_32(&config.peer_key).unwrap().into(),
        peer_endpoint: config.peer_endpoint,
        keepalive_interval: config.keepalive_interval,
        // bind_ip_port: config.bind_ip_port,
        bind_ip_port: bind_ip_port,
    };

    // let ret = rt.block_on(async move {
    
    let notify_shutdown_f1 = notify_shutdown.clone();
    
    let f1 = async move {
        let f = libwgslirpy::run(wg_config, router_config, config.transmit_queue_capacity);
        let mut jh = tokio::spawn(f);
        // let mut rx_shutdown = rx_shutdown;
        info!("Starting wgslirpy");
        loop {
            enum SelectOutcome {
                Returned(Result<anyhow::Result<()>,tokio::task::JoinError>),
                Aborted,
            }
            let ret = tokio::select! {
                x = &mut jh => SelectOutcome::Returned(x),
                // _ = &mut rx_shutdown => SelectOutcome::Aborted,
                _ = notify_shutdown_f1.notified() => SelectOutcome::Aborted,
            };
            match ret {
                SelectOutcome::Returned(Ok(Err(e))) => {
                    error!("Failed to run wgslirpy: {e}");
                    return format!("{e}");
                }
                SelectOutcome::Returned(_) => {
                    error!("Abnormal exit of wgslirpy");
                    return format!("Abnormal exit of wgslirpy");
                }
                SelectOutcome::Aborted => {
                    jh.abort();
                    return "".to_owned();
                }
            }
        }
    };

    let notify_shutdown_f2 = notify_shutdown.clone();

    let f2 = async move {
        let tcp_addr = vec![config.bind_ip_port];
        let udp_addr = bind_ip_port;
        let mut tcp2udp_options = tcp2udp::Options::new(
            tcp_addr,
            udp_addr
        );
        tcp2udp_options.udp_bind_addr = config.peer_endpoint;
        let f = udp_over_tcp::tcp2udp::run(tcp2udp_options);

        // let f = libwgslirpy::run(wg_config, router_config, config.transmit_queue_capacity);
        let mut jh = tokio::spawn(f);
        // let mut rx_shutdown = rx_shutdown;
        info!("Starting tcp2udp");
        loop {
            enum SelectOutcome {
                Returned(Result<Result<Infallible, Tcp2UdpError>, tokio::task::JoinError>),
                Aborted,
            }
            let ret = tokio::select! {
                x = &mut jh => SelectOutcome::Returned(x),
                // _ = &mut rx_shutdown => SelectOutcome::Aborted,
                _ = notify_shutdown_f2.notified() => SelectOutcome::Aborted,
            };
            match ret {
                SelectOutcome::Returned(Ok(Err(e))) => {
                    error!("Failed to run tcp2udp: {e}");
                    return format!("{e}");
                }
                SelectOutcome::Returned(_) => {
                    error!("Abnormal exit of tcp2udp");
                    return format!("Abnormal exit of tcp2udp");
                }
                SelectOutcome::Aborted => {
                    jh.abort();
                    return "".to_owned();
                }
            }
        }
    };

    let ret = rt.block_on(async {
        return futures::join!(f1, f2);
    });

    // let ret = rt.block_on(futures_joined());

    // return env.new_string(ret).unwrap().into_raw()
    return env.new_string(ret.0).unwrap().into_raw()
}

#[no_mangle]
pub extern "system" fn Java_org_vi_1server_wgserver_Native_getSampleConfig(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let sample_config = Config {
        debug: true,
        private_key: "4E4npXgGTLPE/1o25Ewz6WzugXjj9fRA1sIdgGFwR38=".to_owned(),
        peer_key: "c5Hiwoc50CTASEo8DvIcE0g2eJcUsNQdqrQ9ddMlxQo=".to_owned(),
        // peer_key: "LOEtbUZ3a/4JzZR7bNofx3JaWzwije9PTM8YNAuIziU=".to_owned(),
        peer_endpoint: Some("127.0.0.1:9797".parse().unwrap()),
        keepalive_interval: Some(15),
        bind_ip_port: "192.168.12.15:9798".parse().unwrap(),
        // From libwgslirpy: If UDP datagrams are directed at this socket address then attempt to reply to a DNS request internally instead of forwarding the datagram properly
        dns_addr: Some("8.8.8.8:53".parse().unwrap()),
        // From libwgslirpy: If ICMP or ICMPv6 packet is directed at this address, route it to smoltcp's interface (which will reply to ICMP echo requests) instead of dropping it.
        pingable: Some("192.168.24.2".parse().unwrap()),
        mtu: 1420,
        tcp_buffer_size: 65536,
        // incoming_udp: vec![PortForward {
        //     host: "0.0.0.0:8053".parse().unwrap(),
        //     src: Some("99.99.99.99:99".parse().unwrap()),
        //     dst: "10.0.2.15:5353".parse().unwrap(),
        // }],
        incoming_udp: vec![],
        // incoming_tcp: vec![
        //     PortForward {
        //         host: "0.0.0.0:8080".parse().unwrap(),
        //         src: None,
        //         dst: "10.0.2.15:80".parse().unwrap(),
        //     },
        //     PortForward {
        //         host: "0.0.0.0:2222".parse().unwrap(),
        //         src: None,
        //         dst: "10.0.2.15:22".parse().unwrap(),
        //     },
        // ],
        incoming_tcp: vec![],
        transmit_queue_capacity: 128,
    };
    let output = env
        .new_string(toml::to_string_pretty(&sample_config).unwrap())
        .expect("Couldn't create java string!");
    output.into_raw()
}
