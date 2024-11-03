// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! A high-performance raw TCP client implementation with TLS capability.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::prelude::*;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::Mutex,
    task,
    time::sleep,
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, stream::Mode, Error},
    MaybeTlsStream,
};

use crate::tls::tcp_tls;

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type SharedTcpWriter = Arc<Mutex<WriteHalf<MaybeTlsStream<TcpStream>>>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;

/// Configuration for TCP socket connection.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The connection mode {Plain, TLS}.
    pub mode: Mode,
    /// The sequence of bytes which separates lines.
    pub suffix: Vec<u8>,
    /// The Python function to handle incoming messages.
    pub handler: Arc<PyObject>,
    /// The optional heartbeat with period and beat message.
    pub heartbeat: Option<(u64, Vec<u8>)>,
}

/// Creates a TcpStream with the server.
///
/// The stream can be encrypted with TLS or Plain. The stream is split into
/// read and write ends.
/// * The read end is passed to task that keeps receiving
///   messages from the server and passing them to a handler.
/// * The write end is wrapped in an Arc Mutex and used to send messages
///   or heart beats
///
/// The heartbeat is optional and can be configured with an interval and data to
/// send.
///
/// The client uses a suffix to separate messages on the byte stream. It is
/// appended to all sent messages and heartbeats. It is also used the split
/// the received byte stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
struct SocketClientInner {
    config: SocketConfig,
    read_task: task::JoinHandle<()>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedTcpWriter,
}

impl SocketClientInner {
    pub async fn connect_url(config: SocketConfig) -> Result<Self, Error> {
        install_cryptographic_provider();

        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
        } = &config;
        let (reader, writer) = Self::tls_connect_with_server(url, *mode).await?;
        let shared_writer = Arc::new(Mutex::new(writer));

        let handler1 = Python::with_gil(|py| handler.clone_ref(py));
        // Keep receiving messages from socket pass them as arguments to handler
        let read_task = Self::spawn_read_task(reader, handler1, suffix.clone());

        // Optionally create heartbeat task
        let heartbeat_task =
            Self::spawn_heartbeat_task(heartbeat.clone(), shared_writer.clone(), suffix.clone());

        Ok(Self {
            config,
            read_task,
            heartbeat_task,
            writer: shared_writer,
        })
    }

    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        tracing::debug!("Connecting to server");
        let stream = TcpStream::connect(url).await?;
        tracing::debug!("Making TLS connection");
        let request = url.into_client_request()?;
        tcp_tls(&request, mode, stream, None).await.map(split)
    }

    #[must_use]
    pub fn spawn_read_task(
        mut reader: TcpReader,
        handler: PyObject,
        suffix: Vec<u8>,
    ) -> task::JoinHandle<()> {
        // Keep receiving messages from socket pass them as arguments to handler
        task::spawn(async move {
            let mut buf = Vec::new();

            loop {
                match reader.read_buf(&mut buf).await {
                    // Connection has been terminated or vector buffer is completely
                    Ok(0) => {
                        tracing::error!("Cannot read anymore bytes");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Failed with error: {e}");
                        break;
                    }
                    // Received bytes of data
                    Ok(bytes) => {
                        tracing::trace!("Received <binary> {bytes} bytes");

                        // While received data has a line break
                        // drain it and pass it to the handler
                        while let Some((i, _)) = &buf
                            .windows(suffix.len())
                            .enumerate()
                            .find(|(_, pair)| pair.eq(&suffix))
                        {
                            let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                            data.truncate(data.len() - suffix.len());

                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (data.as_slice(),)))
                            {
                                tracing::error!("Call to handler failed: {e}");
                                break;
                            }
                        }
                    }
                };
            }
        })
    }

    /// Optionally spawn a heartbeat task to periodically ping the server.
    pub fn spawn_heartbeat_task(
        heartbeat: Option<(u64, Vec<u8>)>,
        writer: SharedTcpWriter,
        suffix: Vec<u8>,
    ) -> Option<task::JoinHandle<()>> {
        heartbeat.map(|(duration, mut message)| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                message.extend(suffix);
                loop {
                    sleep(duration).await;
                    tracing::debug!("Sending heartbeat");
                    let mut guard = writer.lock().await;
                    match guard.write_all(&message).await {
                        Ok(()) => tracing::debug!("Sent heartbeat"),
                        Err(e) => tracing::error!("Failed to send heartbeat: {e}"),
                    }
                }
            })
        })
    }

    /// Shutdown read task and the connection.
    ///
    /// The client must be explicitly shutdown before dropping otherwise
    /// the connection might still be alive for some time before terminating.
    /// Closing the connection is an async call which cannot be done by the
    /// drop method so it must be done explicitly.
    pub async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        tracing::debug!("Abort read task");
        if !self.read_task.is_finished() {
            self.read_task.abort();
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                tracing::debug!("Abort heartbeat task");
                handle.abort();
            }
        }

        tracing::debug!("Shutdown writer");
        let mut writer = self.writer.lock().await;
        writer.shutdown().await
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update the shared writer and the read and heartbeat tasks.
    ///
    /// TODO: fix error type
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
        } = &self.config;
        tracing::debug!("Reconnecting client");
        let (reader, new_writer) = Self::tls_connect_with_server(url, *mode).await?;

        tracing::debug!("Use new writer end");
        let mut guard = self.writer.lock().await;
        *guard = new_writer;
        drop(guard);

        let handler1 = Python::with_gil(|py| handler.clone_ref(py));
        tracing::debug!("Recreate reader and heartbeat task");
        self.read_task = Self::spawn_read_task(reader, handler1, suffix.clone());
        self.heartbeat_task =
            Self::spawn_heartbeat_task(heartbeat.clone(), self.writer.clone(), suffix.clone());
        Ok(())
    }

    /// Check if the client is still connected.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown. There might be some delay between the connection being closed
    /// and the client detecting it.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.read_task.is_finished()
    }
}

impl Drop for SocketClientInner {
    fn drop(&mut self) {
        if !self.read_task.is_finished() {
            self.read_task.abort();
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
            }
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketClient {
    pub(crate) writer: SharedTcpWriter,
    pub(crate) controller_task: task::JoinHandle<()>,
    pub(crate) disconnect_mode: Arc<AtomicBool>,
    pub(crate) suffix: Vec<u8>,
}

impl SocketClient {
    pub async fn connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> Result<Self, Error> {
        let suffix = config.suffix.clone();
        let inner = SocketClientInner::connect_url(config).await?;
        let writer = inner.writer.clone();
        let disconnect_mode = Arc::new(AtomicBool::new(false));

        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            post_reconnection,
            post_disconnection,
        );

        if let Some(handler) = post_connection {
            Python::with_gil(|py| match handler.call0(py) {
                Ok(_) => tracing::debug!("Called `post_connection` handler"),
                Err(e) => tracing::error!("Error calling `post_connection` handler: {e}"),
            });
        }

        Ok(Self {
            writer,
            controller_task,
            disconnect_mode,
            suffix,
        })
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is not alive.
    pub async fn disconnect(&self) {
        self.disconnect_mode.store(true, Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_disconnected() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        {
            Ok(()) => {
                tracing::debug!("Controller task finished");
            }
            Err(_) => {
                tracing::error!("Timeout waiting for controller task to finish");
            }
        }
    }

    pub async fn send_bytes(&self, data: &[u8]) -> Result<(), std::io::Error> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data).await?;
        writer.write_all(&self.suffix).await
    }

    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        disconnect_mode: Arc<AtomicBool>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            loop {
                sleep(Duration::from_millis(100)).await;

                // Check if client needs to disconnect
                let disconnected = disconnect_mode.load(Ordering::SeqCst);
                match (disconnected, inner.is_alive()) {
                    (false, false) => match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
                            if let Some(ref handler) = post_reconnection {
                                Python::with_gil(|py| match handler.call0(py) {
                                    Ok(_) => tracing::debug!("Called `post_reconnection` handler"),
                                    Err(e) => {
                                        tracing::error!(
                                            "Error calling `post_reconnection` handler: {e}"
                                        );
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!("Reconnect failed {e}");
                            break;
                        }
                    },
                    (true, true) => {
                        tracing::debug!("Shutting down inner client");
                        match inner.shutdown().await {
                            Ok(()) => tracing::debug!("Closed connection"),
                            Err(e) => tracing::error!("Error on `shutdown`: {e}"),
                        }

                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => tracing::debug!("Called `post_disconnection` handler"),
                                Err(e) => {
                                    tracing::error!(
                                        "Error calling `post_disconnection` handler: {e}"
                                    );
                                }
                            });
                        }
                        break;
                    }
                    (true, false) => break,
                    _ => (),
                }
            }
        })
    }
}
