use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::IntoRawFd;
use std::os::fd::RawFd;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

#[derive(Serialize, Deserialize)]
pub struct State {
    listener_raw_fd: RawFd,
    sessions: Vec<Session>,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    client_raw_fd: RawFd,
    peer_addr: std::net::SocketAddr,
}

enum ServerMsg {
    Suspend(tokio::sync::mpsc::Sender<Result<State, String>>),
    Stop(tokio::sync::mpsc::Sender<Result<(), String>>),
    ClientExit(std::net::SocketAddr),
}

enum ClientMsg {
    Suspend(tokio::sync::mpsc::Sender<Result<Session, String>>),
    Stop(tokio::sync::mpsc::Sender<Result<(), String>>),
}

struct Context {
    tx: tokio::sync::mpsc::Sender<ServerMsg>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

static CTX: std::sync::Mutex<Option<Context>> = std::sync::Mutex::new(None);

struct Client {
    rx: tokio::sync::mpsc::Receiver<ClientMsg>,
    server_tx: tokio::sync::mpsc::Sender<ServerMsg>,
    peer_addr: std::net::SocketAddr,
}

impl Client {
    fn new(
        rx: tokio::sync::mpsc::Receiver<ClientMsg>,
        server_tx: tokio::sync::mpsc::Sender<ServerMsg>,
        peer_addr: std::net::SocketAddr,
    ) -> Self {
        Self {
            rx: rx,
            server_tx: server_tx,
            peer_addr: peer_addr,
        }
    }
    async fn run(&mut self, mut stream: tokio::net::TcpStream) {
        //let peer = stream.peer_addr().unwrap();
        eprintln!("Client::run: begin");
        loop {
            let mut buf = [0x00u8; 1024];
            tokio::select! {
                val = stream.read(&mut buf) => {
                    match val {
                        Err(_) => {
                            eprintln!("stream.read error");
                            break;
                        }
                        Ok(n) if n == 0 => {
                            eprintln!("n == 0");
                            break;
                        }
                        Ok(_) => {
                            eprintln!("aaa");
                            let _ = stream.write_all(&buf).await;
                        }
                    }
                }
                val = self.rx.recv() => {
                    match val {
                        Some(ClientMsg::Stop(tx)) => {
                            eprintln!("ClientMsg::Stop");
                            let _ = tx.send(Ok(())).await;
                            break;
                        }
                        Some(ClientMsg::Suspend(tx)) => {
                            eprintln!("ClientMsg::Suspend");
                            let stream = stream.into_std().unwrap();
                            std::mem::forget(stream.try_clone().unwrap());
                            let _ = tx.send(Ok(Session {
                                client_raw_fd: stream.as_raw_fd(),
                                peer_addr: stream.peer_addr().unwrap(),
                            })).await;
                            break;
                        }
                        _ => {
                            eprintln!("client rx.recv val ???");
                            //break;
                        }
                    }
                }
            }
        }
        /*
        let _ = self
            .server_tx
            .send(ServerMsg::ClientExit(peer))
            .await;
            */
    }
}

struct Server {
    rx: tokio::sync::mpsc::Receiver<ServerMsg>,
    tx: tokio::sync::mpsc::Sender<ServerMsg>,
    clients: HashMap<
        std::net::SocketAddr,
        (
            tokio::sync::mpsc::Sender<ClientMsg>,
            tokio::task::JoinHandle<()>,
        ),
    >,
}

impl Server {
    fn new(
        rx: tokio::sync::mpsc::Receiver<ServerMsg>,
        tx: tokio::sync::mpsc::Sender<ServerMsg>,
    ) -> Self {
        Self {
            rx: rx,
            tx: tx,
            clients: HashMap::<
                std::net::SocketAddr,
                (
                    tokio::sync::mpsc::Sender<ClientMsg>,
                    tokio::task::JoinHandle<()>,
                ),
            >::new(),
        }
    }
    async fn run(&mut self, listener: tokio::net::TcpListener) {
        loop {
            tokio::select! {
                val = listener.accept() => {
                    match val {
                        Ok((stream, _)) => {
                            let peer = stream.peer_addr().unwrap();
                            let peer2 = stream.peer_addr().unwrap();
                            let (tx, rx) = tokio::sync::mpsc::channel::<ClientMsg>(1);
                            let server_tx = self.tx.clone();
                            let join_handle = tokio::task::spawn(async move {
                                let mut client = Client::new(rx, server_tx, peer2);
                                client.run(stream).await;
                            });
                            self.clients.insert(peer, (tx, join_handle));
                        }
                        _ => {
                            eprintln!("listener.accept error");
                            break;
                        }
                    }
                }
                val = self.rx.recv() => {
                    match val {
                        Some(ServerMsg::Stop(tx)) => {
                            eprintln!("ServerMsg::Stop");
                            for (_, (client_tx, _)) in &self.clients {
                                let (tx, _) = tokio::sync::mpsc::channel::<Result<(), String>>(1);
                                let _ = client_tx.send(ClientMsg::Stop(tx)).await;
                            }
                            let _ = tx.send(Ok(())).await;
                            break;
                        }
                        Some(ServerMsg::Suspend(tx)) => {
                            eprintln!("ServerMsg::Suspend");
                            let mut sessions =  Vec::<Session>::new();
                            for (_, (client_tx, _)) in &self.clients {
                                let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Session, String>>(1);
                                let _ = client_tx.send(ClientMsg::Suspend(tx)).await;
                                if let Some(Ok(session)) = rx.recv().await {
                                    sessions.push(session);
                                }
                            }
                            let listener = listener.into_std().unwrap();
                            std::mem::forget(listener.try_clone().unwrap());
                            let _ = tx.send(Ok(State {
                                listener_raw_fd: listener.into_raw_fd(),
                                sessions: sessions,
                            })).await;
                            break;
                        }
                        Some(ServerMsg::ClientExit(peer)) => {
                            eprintln!("ServerMsg::ClientExit");
                            self.clients.remove(&peer);
                        }
                        _ => {
                            eprintln!("server rx.recv val ???");
                            break;
                        }
                    }
                }
            }
        }
    }
    async fn start(&mut self) {
        let listener = match tokio::net::TcpListener::bind("0.0.0.0:7777").await {
            Ok(listener) => listener,
            Err(_) => {
                eprintln!("TcpListener::bind error");
                return;
            }
        };
        self.run(listener).await;
    }
    async fn resume(&mut self, raw_fd: RawFd) {
        let listener = unsafe { std::net::TcpListener::from_raw_fd(raw_fd) };
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(_) => {
                eprintln!("TcpListener::from_std error");
                return;
            }
        };
        self.run(listener).await;
    }
}

#[no_mangle]
pub fn start() {
    eprintln!("start!");
    if let Some(ref mut _ctx) = *CTX.lock().unwrap() {
        eprintln!("start ctx exists?");
        return;
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel::<ServerMsg>(1);
    let server_tx = tx.clone();
    let join_handle = std::thread::spawn(move || {
        let mut server = Server::new(rx, server_tx);
        runtime.block_on(server.start())
    });
    let ctx = Context {
        tx: tx,
        join_handle: Some(join_handle),
    };
    *CTX.lock().unwrap() = Some(ctx);
}

#[no_mangle]
pub fn resume(data: std::sync::Arc<std::sync::Mutex<String>>) {
    eprintln!("resume!");
    /*
    if let Some(ref mut _ctx) = *CTX.lock().unwrap() {
        eprintln!("resume ctx exists?");
        return;
    }
    */
    let state: State = match serde_json::from_str(data.lock().unwrap().as_ref()) {
        Ok(state) => state,
        Err(_) => {
            eprintln!("resume serde_json::from_str failed");
            return;
        }
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel::<ServerMsg>(1);
    let server_tx1 = tx.clone();
    let server_tx2 = tx.clone();
    let join_handle = std::thread::spawn(move || {
        let mut server = Server::new(rx, server_tx1);
        runtime.block_on(async move {
            for session in state.sessions {
                let peer = session.peer_addr.clone();
                let peer2 = session.peer_addr.clone();
                let stream = unsafe { std::net::TcpStream::from_raw_fd(session.client_raw_fd) };
                let stream = match tokio::net::TcpStream::from_std(stream) {
                    Ok(stream) => stream,
                    Err(e) => {
                        eprintln!("resume: session.client_raw_fd = {}", session.client_raw_fd);
                        eprintln!("TcpStream::from_std error: {:?}", e);
                        return;
                    }
                };
                let (tx, rx) = tokio::sync::mpsc::channel::<ClientMsg>(1);
                let server_tx = server_tx2.clone();
                let join_handle = tokio::task::spawn(async move {
                    let mut client = Client::new(rx, server_tx, peer2);
                    client.run(stream).await;
                });
                server.clients.insert(peer, (tx, join_handle));
            }
            server.resume(state.listener_raw_fd).await
        })
    });
    let ctx = Context {
        tx: tx,
        join_handle: Some(join_handle),
    };
    *CTX.lock().unwrap() = Some(ctx);
}

#[no_mangle]
pub fn suspend(data: std::sync::Arc<std::sync::Mutex<String>>) {
    eprintln!("suspend!");
    let mut join_handle = None;
    if let Some(ref mut ctx) = *CTX.lock().unwrap() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<State, String>>(1);
        let _ = ctx.tx.blocking_send(ServerMsg::Suspend(tx));
        if let Some(Ok(state)) = rx.blocking_recv() {
            for session in &state.sessions {
                eprintln!("suspend: session.client_raw_fd = {}", session.client_raw_fd);
            }
            *data.lock().unwrap() = serde_json::to_string(&state).unwrap();
        } else {
            eprintln!("rx.blocking_recv error");
        }
        join_handle = ctx.join_handle.take();
    } else {
        eprintln!("lock failed");
    }
    if let Some(join_handle) = join_handle {
        eprintln!("join_handle.join begin");
        let _ = join_handle.join();
        eprintln!("join_handle.join end");
    }
    eprintln!("suspend exit");
}

#[no_mangle]
pub fn stop() {
    eprintln!("stop!");
    let mut join_handle = None;
    if let Some(ref mut ctx) = *CTX.lock().unwrap() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<(), String>>(1);
        let _ = ctx.tx.blocking_send(ServerMsg::Stop(tx));
        if let Some(Ok(())) = rx.blocking_recv() {
        } else {
            eprintln!("rx.blocking_recv error");
        }
        join_handle = ctx.join_handle.take();
    } else {
        eprintln!("lock failed");
    }
    if let Some(join_handle) = join_handle {
        eprintln!("join_handle.join begin");
        let _ = join_handle.join();
        eprintln!("join_handle.join end");
    }
    eprintln!("stop exit");
}
