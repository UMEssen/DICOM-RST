use dicom::core::PrimitiveValue;
use dicom::dictionary_std::uids;
use std::fmt::{Display, Formatter};

/// UI (Unique Identifier) value representation.
pub type UI = String;

/// UL (Unsigned Long) value representation.
pub type UL = u32;

/// US (Unsigned Short) value representation.
pub type US = u16;

/// AE (Application Entity) value representation.
pub type AE = String;

/// Priority (0000,0700) values for DIMSE operations.
#[derive(Debug, Copy, Clone)]
pub enum Priority {
	Low = 0x0002,
	Medium = 0x0000,
	High = 0x0001,
}

impl Default for Priority {
	fn default() -> Self {
		Self::Medium
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum QueryInformationModel {
	Study,
	Patient,
	Worklist,
}

impl Default for QueryInformationModel {
	fn default() -> Self {
		Self::Study
	}
}

impl QueryInformationModel {
	pub const fn as_sop_class(&self) -> &str {
		match self {
			Self::Study => uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND,
			Self::Patient => uids::PATIENT_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND,
			Self::Worklist => uids::MODALITY_WORKLIST_INFORMATION_MODEL_FIND,
		}
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum QueryRetrieveLevel {
	Patient,
	Study,
	Series,
	Image,
	Frame,
}

impl Display for QueryRetrieveLevel {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Patient => write!(f, "PATIENT"),
			Self::Study => write!(f, "STUDY"),
			Self::Series => write!(f, "SERIES"),
			Self::Image => write!(f, "IMAGE"),
			Self::Frame => write!(f, "FRAME"),
		}
	}
}

impl From<QueryRetrieveLevel> for PrimitiveValue {
	fn from(level: QueryRetrieveLevel) -> Self {
		Self::Str(level.to_string())
	}
}
