use std::net::SocketAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::channel::Channel;
use crate::codec::Codec;
use crate::error::Result;

/// Perform a one-off TCP request/response
///
/// Opens a connection, sends the request, receives the response, and closes the connection.
pub async fn request_tcp<Req, Res, C>(addr: SocketAddr, request: &Req, codec: C) -> Result<Res>
where
    Req: Serialize,
    Res: for<'de> Deserialize<'de>,
    C: Codec,
{
    let mut channel = Channel::tcp(addr, codec).await?;
    channel.send(request).await?;
    let response = channel.receive().await?;
    channel.close().await?;
    Ok(response)
}

/// Perform a one-off Unix socket request/response
pub async fn request_unix<Req, Res, C>(
    path: impl AsRef<Path>,
    request: &Req,
    codec: C,
) -> Result<Res>
where
    Req: Serialize,
    Res: for<'de> Deserialize<'de>,
    C: Codec,
{
    let mut channel = Channel::unix(path, codec).await?;
    channel.send(request).await?;
    let response = channel.receive().await?;
    channel.close().await?;
    Ok(response)
}

/// Send a message over TCP without waiting for a response (fire-and-forget)
pub async fn send_tcp<T, C>(addr: SocketAddr, message: &T, codec: C) -> Result<()>
where
    T: Serialize,
    C: Codec,
{
    let mut channel = Channel::tcp(addr, codec).await?;
    channel.send(message).await?;
    channel.close().await?;
    Ok(())
}

/// Send a message over Unix socket without waiting for a response (fire-and-forget)
pub async fn send_unix<T, C>(path: impl AsRef<Path>, message: &T, codec: C) -> Result<()>
where
    T: Serialize,
    C: Codec,
{
    let mut channel = Channel::unix(path, codec).await?;
    channel.send(message).await?;
    channel.close().await?;
    Ok(())
}
