//! ONNX Runtime 1.23.2 official release bytes, paired with ort 2.0.0-rc.11.
pub struct Distribution {
    pub url: &'static str,
    pub archive_sha256: &'static str,
    pub member: &'static str,
    pub name: &'static str,
    pub core_sha256: &'static str,
    pub core_size: usize,
}
pub fn for_target(target: &str) -> Option<Distribution> {
    Some(match target {
        "x86_64-unknown-linux-gnu" => Distribution {
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-x64-1.23.2.tgz",
            archive_sha256: "1fa4dcaef22f6f7d5cd81b28c2800414350c10116f5fdd46a2160082551c5f9b",
            member: "onnxruntime-linux-x64-1.23.2/lib/libonnxruntime.so.1.23.2",
            name: "libonnxruntime.so.1.23.2",
            core_sha256: "13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b",
            core_size: 22326072,
        },
        "aarch64-unknown-linux-gnu" => Distribution {
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-aarch64-1.23.2.tgz",
            archive_sha256: "7c63c73560ed76b1fac6cff8204ffe34fe180e70d6582b5332ec094810241e5c",
            member: "onnxruntime-linux-aarch64-1.23.2/lib/libonnxruntime.so.1.23.2",
            name: "libonnxruntime.so.1.23.2",
            core_sha256: "648ffa64fbe027ae27139109410900cf776a030dec2dbbac51053318cc44c286",
            core_size: 18693384,
        },
        "aarch64-apple-darwin" => Distribution {
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-arm64-1.23.2.tgz",
            archive_sha256: "b4d513ab2b26f088c66891dbbc1408166708773d7cc4163de7bdca0e9bbb7856",
            member: "onnxruntime-osx-arm64-1.23.2/lib/libonnxruntime.1.23.2.dylib",
            name: "libonnxruntime.1.23.2.dylib",
            core_sha256: "d306d2bc768540766c7ed8a1e0ff05d2870c77a934ebeee4a7bafa1b732ef299",
            core_size: 35138784,
        },
        "x86_64-apple-darwin" => Distribution {
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-x86_64-1.23.2.tgz",
            archive_sha256: "d10359e16347b57d9959f7e80a225a5b4a66ed7d7e007274a15cae86836485a6",
            member: "onnxruntime-osx-x86_64-1.23.2/lib/libonnxruntime.1.23.2.dylib",
            name: "libonnxruntime.1.23.2.dylib",
            core_sha256: "8c9c78de65ea3786f987c0d980e9c1b13a3a5fbc6b3e2965ba05b450e6e4c054",
            core_size: 39742608,
        },
        "x86_64-pc-windows-msvc" => Distribution {
            url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-win-x64-1.23.2.zip",
            archive_sha256: "0b38df9af21834e41e73d602d90db5cb06dbd1ca618948b8f1d66d607ac9f3cd",
            member: "onnxruntime-win-x64-1.23.2/lib/onnxruntime.dll",
            name: "onnxruntime.dll",
            core_sha256: "dec964ab1ee36cc9b0ae247d13b376627992fc57dec0454354017ab8fd84f1ea",
            core_size: 14186016,
        },
        _ => return None,
    })
}
