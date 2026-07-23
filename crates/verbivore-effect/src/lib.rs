//! Effect validation: before/after screenshot pair in, verdict out — meaningful change
//! or ambient noise. Runs after every write verb; must beat an SSIM-threshold baseline
//! to earn its model (see SPEC success criteria).
