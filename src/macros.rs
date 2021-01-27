macro_rules! impl_read_num {
    ($ty:ident, le: $read_le:ident, be: $read_be:ident) => {
        impl_read_num!($ty, stringify!($ty), le: $read_le, be: $read_be);
    };
    ($ty:ident, $ty_str:expr, le: $read_le:ident, be: $read_be:ident) => {
        #[doc = "Read a little-endian encoded `"]
        #[doc = $ty_str]
        #[doc = "`."]
        ///
        /// # Errors
        ///
        /// Returns an error if there is not sufficient input left to read.
        pub fn $read_le(&mut self) -> Result<$ty, E>
        where
            E: From<ExpectedLength<'i>>,
        {
            self.try_advance(|input| {
                let (arr, next) = input.split_array(concat!("read little-endian ", $ty_str))?;
                let number = $ty::from_le_bytes(arr);
                Ok((number, next))
            })
        }

        #[doc = "Read a big-endian encoded `"]
        #[doc = $ty_str]
        #[doc = "`."]
        ///
        /// # Errors
        ///
        /// Returns an error if there is not sufficient input left to read.
        pub fn $read_be(&mut self) -> Result<$ty, E>
        where
            E: From<ExpectedLength<'i>>,
        {
            self.try_advance(|input| {
                let (arr, next) = input.split_array(concat!("read big-endian ", $ty_str))?;
                let number = $ty::from_be_bytes(arr);
                Ok((number, next))
            })
        }
    };
}
