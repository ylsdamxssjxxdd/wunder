// 统一处理退出信号，便于优雅停机。
use tracing::info;

pub async fn shutdown_signal() {
    // 同时监听 Ctrl+C 与 SIGTERM，保证容器关闭时优雅退出。
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            eprintln!("监听退出信号失败: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut stream = signal(SignalKind::terminate()).expect("无法注册 SIGTERM 监听器");
        stream.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("收到退出信号，准备关闭服务。");
}
