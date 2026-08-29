use crate::constant::*;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::view::attr_view::AttrView;
use crate::view::mft_view::MftRecordView;
use alloc::vec::Vec;
use rimio::RimIO;
use rimio::errors::RimIOResult;

/// Find the LCN of the cluster bitmap from its MFT record
pub fn find_bitmap_lcn<IO: RimIO + ?Sized>(io: &mut IO, meta: &NtfsMeta) -> RimIOResult<u64> {
    let record_buf = mft::read_record(io, meta, MFT_RECORD_BITMAP)?;

    let view = MftRecordView::new(&record_buf)
        .map_err(|_| rimio::errors::RimIOError::Invalid("Failed to parse $Bitmap MFT record"))?;

    let attr = view
        .find(ATTR_DATA)
        .map_err(|_| rimio::errors::RimIOError::Invalid("Malformed $Bitmap attribute"))?
        .ok_or(rimio::errors::RimIOError::Invalid(
            "Could not find $DATA in $Bitmap",
        ))?;

    let attr_view = attr
        .as_view()
        .map_err(|_| rimio::errors::RimIOError::Invalid("Malformed $Bitmap attribute view"))?;

    match attr_view {
        AttrView::NonResident { runlist, .. } => {
            let run = runlist
                .iter()
                .next()
                .ok_or(rimio::errors::RimIOError::Invalid(
                    "Empty datarun for $Bitmap",
                ))?;
            run.lcn
                .ok_or(rimio::errors::RimIOError::Invalid("Sparse run for $Bitmap"))
        }
        AttrView::Resident { .. } => Err(rimio::errors::RimIOError::Invalid(
            "$Bitmap $DATA is resident, unexpected",
        )),
    }
}

/// Write the cluster bitmap bytes to its volume location
pub fn write_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &NtfsMeta,
    bitmap_bytes: &[u8],
) -> RimIOResult {
    let lcn = find_bitmap_lcn(io, meta)?;
    let offset = meta.lcn_to_offset(lcn);
    io.write_at(offset, bitmap_bytes)?;
    Ok(())
}

/// Read the cluster bitmap bytes from its volume location
pub fn read_bitmap<IO: RimIO + ?Sized>(io: &mut IO, meta: &NtfsMeta) -> RimIOResult<Vec<u8>> {
    let lcn = find_bitmap_lcn(io, meta)?;
    let offset = meta.lcn_to_offset(lcn);
    let mut buf = vec![0u8; meta.bitmap_size_bytes as usize];
    io.read_at(offset, &mut buf)?;
    Ok(buf)
}
