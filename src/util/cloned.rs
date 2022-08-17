/// Transform (&T, &U) -> (T, U)
pub trait TupleCloned {
    /// The cloned version of a tuple
    type Cloned: 'static;

    /// Clone the items in the tuple
    fn cloned(self) -> Self::Cloned;
}

impl<T> TupleCloned for &T
where
    T: Clone + 'static,
{
    type Cloned = T;

    fn cloned(self) -> Self::Cloned {
        (self).clone()
    }
}

impl<T> TupleCloned for &mut T
where
    T: Clone + 'static,
{
    type Cloned = T;

    fn cloned(self) -> Self::Cloned {
        (self).clone()
    }
}
