/// Transform (&T, &U) -> (T, U)
pub trait TupleCloned {
    type Cloned: 'static;

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
