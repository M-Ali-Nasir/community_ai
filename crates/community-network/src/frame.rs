use quinn::{RecvStream, SendStream};

use community_core::{CommunityError, Result};
use community_protocol::{MeshFrame, MAX_FRAME_BYTES};

pub async fn write_frame(send: &mut SendStream, frame: &MeshFrame) -> Result<()> {
    let json = serde_json::to_vec(frame)
        .map_err(|e| CommunityError::Network(format!("json encode: {e}")))?;
    if json.len() > MAX_FRAME_BYTES {
        return Err(CommunityError::Network("frame too large".into()));
    }
    let len = (json.len() as u32).to_be_bytes();
    send.write_all(&len)
        .await
        .map_err(|e| CommunityError::Network(format!("write len: {e}")))?;
    send.write_all(&json)
        .await
        .map_err(|e| CommunityError::Network(format!("write body: {e}")))?;
    Ok(())
}

pub async fn read_frame(recv: &mut RecvStream, max_bytes: usize) -> Result<MeshFrame> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| CommunityError::Network(format!("read len: {e}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > max_bytes {
        return Err(CommunityError::Network(format!(
            "invalid frame length {len}"
        )));
    }
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .map_err(|e| CommunityError::Network(format!("read body: {e}")))?;
    serde_json::from_slice(&body).map_err(|e| CommunityError::Network(format!("json decode: {e}")))
}
