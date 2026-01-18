// rimfs/core/validate.rs
pub trait Validate<M> {
    type Err;
    fn neutralized(&self) -> Self;
    fn validate(&self, meta: &M) -> Result<(), Self::Err>;
}
