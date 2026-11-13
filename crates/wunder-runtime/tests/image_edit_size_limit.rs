//! 图像尺寸上限回归：编辑大图时必须按上限等比缩小输入图，并按缩小后的尺寸请求上游。
//!
//! 这里用一个本地 mock 的 `/v1/images/edits` 服务抓请求体，验证两件事：
//! 1. multipart 里的 `size` 字段是缩小后的尺寸；
//! 2. 真正上传的图片字节尺寸也被缩小（否则上游仍按原图尺寸吃显存）。

use base64::Engine;
use image::RgbaImage;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wunder_server::config::{Config, LlmModelConfig};
use wunder_server::multimodal_models::{
    generate_image, ImageGenerationRequest, ImageInputFile,
};

struct CapturedRequest {
    content_type: String,
    body: Vec<u8>,
}

impl CapturedRequest {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// 取请求体里第一个 PNG 的宽高（IHDR）。
    fn first_png_size(&self) -> Option<(u32, u32)> {
        let start = self
            .body
            .windows(8)
            .position(|window| window == b"\x89PNG\r\n\x1a\n")?;
        let header = self.body.get(start + 16..start + 24)?;
        let width = u32::from_be_bytes(header[0..4].try_into().ok()?);
        let height = u32::from_be_bytes(header[4..8].try_into().ok()?);
        Some((width, height))
    }
}

async fn spawn_mock_edit_server() -> (String, tokio::sync::oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16384];
        let header_end = loop {
            let read = stream.read(&mut chunk).await.expect("read request");
            if read == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..read]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        let content_type = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case("content-type")
                    .then(|| value.trim().to_string())
            })
            .unwrap_or_default();
        while buf.len() < header_end + 4 + content_length {
            let read = stream.read(&mut chunk).await.expect("read body");
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
        }
        let body = buf[header_end + 4..].to_vec();
        let _ = tx.send(CapturedRequest {
            content_type,
            body,
        });

        let payload = format!(
            "{{\"data\":[{{\"b64_json\":\"{}\"}}]}}",
            base64::engine::general_purpose::STANDARD.encode(solid_png(8, 8))
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        stream.flush().await.ok();
    });
    (format!("http://{addr}/v1"), rx)
}

fn solid_png(width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([30, 90, 200, 255]),
    ));
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode png");
    bytes
}

fn image_config(base_url: String, max_size: Option<&str>) -> Config {
    let mut config = Config::default();
    config.llm.models.insert(
        "image-demo".to_string(),
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            base_url: Some(base_url),
            model: Some("image-demo".to_string()),
            model_type: Some("image".to_string()),
            image_max_size: max_size.map(str::to_string),
            timeout_s: Some(30),
            ..Default::default()
        },
    );
    config.llm.default_image = Some("image-demo".to_string());
    config
}

fn edit_request(path_label: &str, width: u32, height: u32) -> ImageGenerationRequest {
    ImageGenerationRequest {
        prompt: "replace the background with a night scene".to_string(),
        model_name: Some("image-demo".to_string()),
        size: None,
        output_format: None,
        negative_prompt: None,
        num_inference_steps: None,
        guidance_scale: None,
        seed: None,
        input_images: vec![ImageInputFile {
            bytes: solid_png(width, height).into(),
            filename: format!("{path_label}.png"),
            content_type: "image/png".to_string(),
        }],
        mask_image: None,
        reference_image: None,
        strength: None,
        true_cfg_scale: None,
        output_compression: None,
        layers: None,
        resolution: None,
    }
}

#[tokio::test]
async fn edit_upload_is_downscaled_and_size_follows_the_limit() {
    let (base_url, captured) = spawn_mock_edit_server().await;
    let config = image_config(base_url, Some("1024x1024"));

    let response = generate_image(&config, edit_request("big", 3000, 1500))
        .await
        .expect("image edit request should succeed");

    let captured = captured.await.expect("captured request");
    assert!(captured.content_type.contains("multipart/form-data"));
    let body = captured.text();
    assert!(
        body.contains("1024x512"),
        "size field should carry the scaled size"
    );
    assert_eq!(
        captured.first_png_size(),
        Some((1024, 512)),
        "uploaded image must already be scaled down"
    );
    assert_eq!(response.size.as_deref(), Some("1024x512"));
    assert_eq!(
        response.resized_input.as_deref(),
        Some("3000x1500->1024x512")
    );
}

#[tokio::test]
async fn edit_without_configured_limit_keeps_source_image() {
    let (base_url, captured) = spawn_mock_edit_server().await;
    let config = image_config(base_url, None);

    let response = generate_image(&config, edit_request("plain", 2000, 1000))
        .await
        .expect("image edit request should succeed");

    let captured = captured.await.expect("captured request");
    assert_eq!(captured.first_png_size(), Some((2000, 1000)));
    assert_eq!(response.size, None);
    assert_eq!(response.resized_input, None);
}

#[allow(dead_code)]
fn assert_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Arc<CapturedRequest>>();
}
