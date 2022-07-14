/// Helper trait to convert a tuple to a closure with distinct arguments
pub trait TupleArgs {
    type Func;
    type Output;

    fn execute(&self, func: Self::Func) -> Self::Output;
}

impl<T, U> TupleArgs for (T, U) {
    type Func = Box<dyn Fn(&T, &U) -> Self::Output>;
    type Output = ();

    fn execute(&self, _: Self::Func) -> Self::Output {
        todo!()
    }
}
