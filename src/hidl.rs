use crate::aidl::{self, AidlError, AidlFile};

pub fn parse(source: &str) -> Result<AidlFile, AidlError> {
    aidl::parse(source)
}

pub fn generate_rust(file: &AidlFile) -> String {
    aidl::generate_hidl_rust(file)
}
