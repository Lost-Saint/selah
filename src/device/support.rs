//! Per-model support honesty: cataloged vs evidenced vs maintainer-verified.

use super::DeviceModel;

/// How strongly Selah can claim this model works.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportLevel {
    /// Maintainer hardware on file (`docs/protocol.md` table).
    VerifiedPrimary,
    /// Externally evidenced mapping (BiD/MixiD), no maintainer audible proof.
    ReferenceDerived,
    /// Known USB ID only; unavailable controls stay hidden.
    Cataloged,
}

/// Maps a catalog model to its support level.
///
/// Today only iD14 MKII (`0x0008`) is maintainer-verified; iD24 (`0x000d`)
/// has an evidenced routing map; everything else is catalog-only.
#[must_use]
pub const fn support_level(model: &DeviceModel) -> SupportLevel {
    match model.product_id {
        0x0008 => SupportLevel::VerifiedPrimary,
        0x000d => SupportLevel::ReferenceDerived,
        _ => SupportLevel::Cataloged,
    }
}

/// Short UI-safe label; never claims verification it does not have.
#[must_use]
pub const fn support_label(level: SupportLevel) -> &'static str {
    match level {
        SupportLevel::VerifiedPrimary => "Verified on iD14 MKII",
        SupportLevel::ReferenceDerived => "Reference-derived",
        SupportLevel::Cataloged => "Cataloged — unverified",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn id14_mkii_is_the_verified_primary() {
        let model = crate::device::supported_device(0x0008).unwrap();
        assert_eq!(
            super::support_level(model),
            super::SupportLevel::VerifiedPrimary
        );
    }

    #[test]
    fn id24_is_reference_derived_and_unmapped_models_are_cataloged() {
        let id24 = crate::device::supported_device(0x000d).unwrap();
        assert_eq!(
            super::support_level(id24),
            super::SupportLevel::ReferenceDerived
        );
        let unmapped = crate::device::supported_device(0x0003).unwrap();
        assert_eq!(
            super::support_level(unmapped),
            super::SupportLevel::Cataloged
        );
    }
}
