use std::fmt;
use std::io::Write;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use flume::Receiver;

#[derive(Debug)]
pub enum Error {
    BindSocketFailed(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::BindSocketFailed(_e) => write!(f, "Failed to bind listening socket"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::BindSocketFailed(e) => Some(e),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

pub struct SpectrumFeedServer {
    listen_socket: TcpListener,
    spectrum_rx: Receiver<String>,
}

impl SpectrumFeedServer {
    pub fn new(listen_addr: impl ToSocketAddrs, spectrum_rx: Receiver<String>) -> Result<Self> {
        let listen_socket =
            std::net::TcpListener::bind(listen_addr).map_err(Error::BindSocketFailed)?;
        log::info!(
            "SpectrumFeedServer listening on {}",
            listen_socket.local_addr().unwrap()
        );

        Ok(Self {
            listen_socket,
            spectrum_rx,
        })
    }

    pub fn run(self) {
        let (new_socket_tx, new_socket_rx) = flume::bounded::<TcpStream>(100);

        // Spawn a thread to listen for incoming connections
        std::thread::spawn(move || {
            for socket in self.listen_socket.incoming() {
                let socket = match socket {
                    Ok(socket) => socket,
                    Err(e) => {
                        log::error!("Failed to accept incoming connection: {:?}", e);
                        continue;
                    }
                };
                if let Err(flume::TrySendError::Disconnected(_)) = new_socket_tx.try_send(socket) {
                    break;
                }
            }
            log::debug!("SpectrumFeedServer listen thread exiting");
        });

        let mut sockets = Vec::new();
        while !new_socket_rx.is_disconnected() {
            // Wait for a new spectrum reading
            let Ok(spectrum) = self.spectrum_rx.recv() else {
                break;
            };

            // Check if there are any new clients
            for new_socket in new_socket_rx.drain() {
                log::info!(
                    "New client connected from {}",
                    new_socket.peer_addr().unwrap()
                );
                sockets.push(new_socket);
            }

            // Send the spectrum to all connected clients. Drop all closed sockets
            sockets.retain_mut(|socket: &mut TcpStream| {
                if let Err(e) = socket.write_all(spectrum.as_bytes()) {
                    log::error!("Failed to send spectrum data: {:?}", e);
                    false
                } else {
                    true
                }
            });
        }
        log::debug!("SpectrumFeedServer send thread exiting");
    }
}
