mod tests {
    use byte::ctx::Endian;
    use byte::{TryRead, TryWrite};
    use byte_derive::{TryRead, TryWrite};
    use std::fmt::Debug;

    fn test_try_write<T: TryWrite<Endian>>(obj: T, exp_bytes: &[u8], ctx: Endian) {
        let mut bytes = [0u8; 1024];

        let size = obj.try_write(bytes.as_mut_slice(), ctx).unwrap();
        assert_eq!(size, exp_bytes.len());
        assert_eq!(&bytes[0..size], exp_bytes);
    }

    fn test_try_read<'a, T: TryRead<'a, Endian> + PartialEq + Debug>(
        bytes: &'a [u8],
        exp_obj: T,
        ctx: Endian,
    ) {
        let (obj, size) = T::try_read(bytes, ctx).unwrap();
        assert_eq!(size, bytes.len());
        assert_eq!(obj, exp_obj);
    }

    fn test_write_and_read<
        'a,
        T: TryWrite<Endian> + TryRead<'a, Endian> + PartialEq + Debug + Clone,
    >(
        obj: T,
        exp_bytes: &'a [u8],
        ctx: Endian,
    ) {
        test_try_write(obj.clone(), exp_bytes, ctx);
        test_try_read(exp_bytes, obj.clone(), ctx);
    }

    #[test]
    fn test_simple_struct() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test(pub u16);

        test_write_and_read(Test(512), &[0, 2], Endian::Little);
    }

    #[test]
    fn test_struct() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test {
            pub a: u8,
            pub b: i16,
        }

        let t = Test { a: 1, b: 512 };

        test_try_write(t.clone(), &[1, 0, 2], Endian::Little);
        test_try_write(t.clone(), &[1, 2, 0], Endian::Big);
        test_try_read(&[14, 0, 2], Test { a: 14, b: 512 }, Endian::Little);
    }

    #[test]
    fn test_simple_option() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test {
            #[byte(ctx = ())]
            pub flag: bool,
            #[byte(parse_if = flag == true)]
            pub option: Option<u8>,
        }

        let t_none = Test {
            flag: false,
            option: None,
        };

        test_write_and_read(t_none, &[0], Endian::Little);

        let t_some = Test {
            flag: true,
            option: Some(2),
        };

        test_write_and_read(t_some, &[255, 2], Endian::Little);
    }

    #[test]
    fn test_complex_option() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Inner {
            pub a: u8,
            pub b: i16,
        }

        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test {
            #[byte(ctx = ())]
            pub flag: bool,
            #[byte(parse_if = flag == true)]
            pub option: Option<Inner>,
        }

        let t_none = Test {
            flag: false,
            option: None,
        };

        test_write_and_read(t_none, &[0], Endian::Little);

        let t_some = Test {
            flag: true,
            option: Some(Inner { a: 8, b: 512 }),
        };

        test_write_and_read(t_some, &[255, 8, 0, 2], Endian::Little);
    }

    #[test]
    fn test_array() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test(pub [i16; 2]);

        test_write_and_read(Test([512, 512]), &[0, 2, 0, 2], Endian::Little);
    }

    #[test]
    fn test_tuple() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test(pub (u8, i16));

        test_write_and_read(Test((14, 512)), &[14, 0, 2], Endian::Little);
    }

    #[test]
    fn test_nested() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Inner {
            pub a: u8,
        }
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Outer {
            pub b: u8,
            pub c: Inner,
        }

        let outer = Outer {
            b: 65,
            c: Inner { a: 70 },
        };

        test_write_and_read(outer, &[65, 70], Endian::Little);
    }

    #[test]
    fn test_enum() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        #[repr(u8)]
        enum Test {
            A = 0x14,
            B = 0x18,
            C(u8) = 0x20,
        }

        test_write_and_read(Test::A, &[0x14], Endian::Little);
        test_write_and_read(Test::B, &[0x18], Endian::Little);
        test_write_and_read(Test::C(8), &[0x20, 8], Endian::Little);
    }

    #[test]
    fn test_vec() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test {
            #[byte(len = u8)]
            pub v: heapless::Vec<u16, 32>,
            pub a: u8,
        }

        let mut vec = heapless::Vec::<u16, 32>::new();
        vec.push(512);
        vec.push(512);
        let t = Test { v: vec, a: 16 };

        test_write_and_read(t, &[2, 0, 2, 0, 2, 16], Endian::Little);
        test_write_and_read(
            Test {
                v: heapless::Vec::new(),
                a: 14,
            },
            &[0, 14],
            Endian::Little,
        );

        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test2 {
            #[byte(len = u8)]
            pub v: Vec<u16>,
        }

        let mut vec = Vec::<u16>::new();
        vec.push(512);
        vec.push(512);
        let t = Test2 { v: vec };

        test_write_and_read(t, &[2, 0, 2, 0, 2], Endian::Little);
    }

    #[test]
    fn test_vec_no_primitive() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Inner {
            pub a: u8,
        }

        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        struct Test {
            #[byte(len = u8)]
            pub v: heapless::Vec<Inner, 32>,
            pub a: u8,
        }

        let mut vec = heapless::Vec::<Inner, 32>::new();
        vec.push(Inner { a: 12 }).unwrap();
        vec.push(Inner { a: 24 }).unwrap();

        let t = Test { v: vec, a: 36 };

        test_write_and_read(t, &[2, 12, 24, 36], byte::LE);
    }

    #[test]
    fn test_enum_vec() {
        #[derive(Debug, Clone, PartialEq, TryRead, TryWrite)]
        #[repr(u8)]
        pub enum Test {
            Item(#[byte(len = u8)] heapless::Vec<u8, 32>) = 0x05,
            Other = 0x4,
        }

        let vec = heapless::Vec::<u8, 32>::from_slice(&[14]).unwrap();
        let t = Test::Item(vec);

        test_write_and_read(t, &[5, 1, 14], Endian::Little);
    }

    #[test]
    fn test_empty_vec() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Test {
            #[byte(len = u8)]
            pub v: heapless::Vec<u8, 32>,
            pub a: u8,
        }

        let t = Test {
            v: heapless::Vec::new(),
            a: 14,
        };

        test_write_and_read(t, &[0, 14], Endian::Little);
    }

    #[test]
    fn test_vec_no_len() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Test {
            pub a: u8,
            pub v: heapless::Vec<u8, 32>,
        }

        let t = Test {
            a: 14,
            v: heapless::Vec::from_array([1, 2, 3]),
        };

        test_write_and_read(t, &[14, 1, 2, 3], Endian::Little);
    }

    #[test]
    fn test_ignore() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Test {
            pub a: u8,
            #[byte(ignore = true)]
            pub b: u8,
            pub c: u8
        }

        let t = Test {
            a: 1,
            b: 2,
            c: 3,
        };

        test_try_write(t, &[1, 3], Endian::Little);
        test_try_read(&[1, 3], Test { a: 1, b: 0, c: 3 }, Endian::Little);
    }


    #[test]
    fn test_ignore_default() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Test {
            pub a: u8,
            #[byte(ignore = true, default = 42)]
            pub b: u8,
            pub c: u8
        }

        let t = Test {
            a: 1,
            b: 2,
            c: 3,
        };

        test_try_write(t, &[1, 3], Endian::Little);
        test_try_read(&[1, 3], Test { a: 1, b: 42, c: 3 }, Endian::Little);
    }

    #[test]
    fn test_no_tag() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        #[byte(no_tag = true)]
        #[repr(u16)]
        pub enum Test {
            A(u8) = 1,
            B(u8) = 2,
            C(u8) = 3,
        }

        test_try_write(Test::B(123), &[123], Endian::Little);
    }

    #[test]
    fn test_option_no_parse_if() {
        #[derive(TryRead, TryWrite, Clone, PartialEq, Debug)]
        pub struct Test {
            pub a: u8,
            pub o: Option<u16>,
            pub c: u8,
        }

        test_write_and_read(Test {
            a: 6,
            o: None,
            c: 11,
        }, &[6, 0, 11], Endian::Little);

        test_write_and_read(Test {
            a: 6,
            o: Some(42),
            c: 11,
        }, &[6, 0xff, 42, 00, 11], Endian::Little);
    }
}
