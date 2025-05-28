use crate::api::wado::{ImageQuality, RenderedRequest, Viewport, Window};
use anyhow::bail;
use dicom::dictionary_std::tags;
use dicom::object::{DefaultDicomObject, FileDicomObject, InMemDicomObject};
use dicom_pixeldata::image::{imageops, DynamicImage};
use dicom_pixeldata::{ConvertOptions, PixelDecoder, VoiLutOption, WindowLevel};
use futures::{Stream, StreamExt};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use serde::{Deserialize, Deserializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, instrument, trace, warn};

#[derive(Debug, Error)]
pub enum RenderingError {
	#[error(transparent)]
	PixelData(#[from] dicom_pixeldata::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderingOptions {
	pub media_type: RenderedMediaType,
	pub quality: Option<ImageQuality>,
	pub viewport: Option<Viewport>,
	pub window: Option<Window>,
}

pub async fn render_instances<S>(
	dicom_stream: &mut S,
	options: &RenderingOptions,
) -> anyhow::Result<Vec<u8>>
where
	S: Stream<Item = Arc<FileDicomObject<InMemDicomObject>>> + Unpin,
{
	while let Some(dicom_object) = dicom_stream.next().await {
		if options.media_type.category() == ResourceCategory::SingleFrameImage {
			if dicom_object.element(tags::PIXEL_DATA).is_err() {
				continue;
			}
			let mut image = decode_single_frame_image(&dicom_object, options.window.as_ref())?;
			if let Some(viewport) = &options.viewport {
				image = apply_viewport(&image, viewport);
			}

			let render_output = render_single_frame_image(&image, options)?;
			return Ok(render_output);
		}

		// TODO: Multi-frame images, videos and text
		bail!("unsupported rendered media type: `{}`", &options.media_type);
	}

	bail!("empty stream: nothing to render")
}

fn decode_single_frame_image(
	dicom_object: &DefaultDicomObject,
	window: Option<&Window>,
) -> anyhow::Result<DynamicImage> {
	let pixel_data = dicom_object.decode_pixel_data()?;

	#[allow(clippy::option_if_let_else)]
	let options = match window {
		Some(windowing) => ConvertOptions::new()
			.with_voi_lut(VoiLutOption::Custom(WindowLevel {
				center: windowing.center,
				width: windowing.width,
			}))
			.force_8bit(),
		None => ConvertOptions::default().force_8bit(),
	};

	let image = pixel_data.to_dynamic_image_with_options(0, &options)?;
	Ok(image)
}

/// Renders the instance as an image using the options provided in the [RenderingOptions].
///
/// This supports the following rendered media types:
/// - `image/jpeg`
/// - `image/png`
/// - `image/gif`
fn render_single_frame_image(
	single_frame_image: &DynamicImage,
	options: &RenderingOptions,
) -> anyhow::Result<Vec<u8>> {
	let mut render_buffer = Vec::new();

	match options.media_type {
		RenderedMediaType::Jpeg => {
			let encoder = JpegEncoder::new_with_quality(
				&mut render_buffer,
				options.quality.unwrap_or_default().into(),
			);
			single_frame_image.write_with_encoder(encoder)?;
		}
		RenderedMediaType::Png => {
			let encoder = PngEncoder::new_with_quality(
				&mut render_buffer,
				CompressionType::default(),
				FilterType::default(),
			);
			single_frame_image.write_with_encoder(encoder)?;
		}
		RenderedMediaType::Gif => unimplemented!(),
	}

	Ok(render_buffer)
}

#[instrument(skip_all)]
pub fn render(
	dicom_file: &FileDicomObject<InMemDicomObject>,
	request: &RenderedRequest,
) -> Result<DynamicImage, RenderingError> {
	trace!(
		sop_instance_uid = dicom_file.meta().media_storage_sop_instance_uid(),
		"Rendering DICOM file"
	);

	let pixel_data = dicom_file.decode_pixel_data()?;

	// Convert the pixel data to an image
	#[allow(clippy::option_if_let_else)]
	let options = match &request.parameters.window {
		Some(windowing) => ConvertOptions::new()
			.with_voi_lut(VoiLutOption::Custom(WindowLevel {
				center: windowing.center,
				width: windowing.width,
			}))
			.force_8bit(),
		None => ConvertOptions::default().force_8bit(),
	};

	let mut image = pixel_data.to_dynamic_image_with_options(0, &options)?;

	if let Some(viewport) = &request.parameters.viewport {
		image = apply_viewport(&image, viewport);
	}

	Ok(image)
}

/// 1. Crop our image to the source rectangle
/// 2. Scale the cropped image to the viewport size
/// 3. Center the scaled image on a new canvas of the viewport size
fn apply_viewport(image: &DynamicImage, viewport: &Viewport) -> DynamicImage {
	let scaled = image
		.crop_imm(
			viewport.source_xpos.unwrap_or(0),
			viewport.source_ypos.unwrap_or(0),
			viewport.source_width.unwrap_or_else(|| image.width()),
			viewport.source_height.unwrap_or_else(|| image.height()),
		)
		.thumbnail(viewport.viewport_width, viewport.viewport_height);

	let mut canvas = DynamicImage::new(
		viewport.viewport_width,
		viewport.viewport_height,
		scaled.color(),
	);

	let dx = (canvas.width() - scaled.width()) / 2;
	let dy = (canvas.height() - scaled.height()) / 2;
	imageops::overlay(&mut canvas, &scaled, i64::from(dx), i64::from(dy));

	canvas
}

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum RenderedMediaType {
	#[default]
	Jpeg,
	Png,
	Gif,
}

impl<'de> Deserialize<'de> for RenderedMediaType {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let s = String::deserialize(deserializer)?;
		s.parse().map_err(serde::de::Error::custom)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCategory {
	SingleFrameImage,
	MultiFrameImage,
	Video,
	Text,
}

impl Display for RenderedMediaType {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl RenderedMediaType {
	pub const fn category(self) -> ResourceCategory {
		match self {
			Self::Jpeg | Self::Png | Self::Gif => ResourceCategory::SingleFrameImage,
		}
	}

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Jpeg => "image/jpeg",
			Self::Png => "image/png",
			Self::Gif => "image/gif",
		}
	}
}

#[derive(Debug, Error)]
#[error("`{0}` is not a supported rendered media type")]
pub struct ParseRenderedMediaTypeError(String);

impl FromStr for RenderedMediaType {
	type Err = ParseRenderedMediaTypeError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"image/png" => Ok(Self::Png),
			"image/jpeg" => Ok(Self::Jpeg),
			"image/gif" => Ok(Self::Gif),
			_ => Err(ParseRenderedMediaTypeError(s.to_owned())),
		}
	}
}
