//! エラー enum を量産するための宣言マクロ。全レイヤーのエラーはこの3つのどれかで定義されている。
//!
//! 生成されるもの: `#[derive(Debug, PartialEq, Eq, Clone)]` 付きの enum と、
//! `Code` / `Cause` / `ErrorTypeProvider`（`common::traits`）の実装。
//!
//! | マクロ | 用途 | variant の形 | `cause()` |
//! |---|---|---|---|
//! | `leaf_error!` | 末端のエラー（詳細メッセージ不要） | `Variant` | 常に `None` |
//! | `leaf_detail_error!` | 末端のエラー（詳細メッセージあり） | `Variant(String)` | `Some(msg)` |
//! | `parent_error!` | 下位エラーを束ねる | `Variant(ChildError)` | 子に委譲 |
//!
//! `parent_error!` は各子エラーからの `From` も生成するので、`?` でそのまま上位に伝播できる。
//! エラーの木構造: leaf → サービス層 parent → ユースケース層 parent → `ApplicationError`
//! → `AppError`（HTTP レスポンス）。HTTP ステータスとエラーコードは常に leaf で決まる。
//!
//! 使用例:
//! ```ignore
//! leaf_error!(
//!     pub enum FooError {
//!         NotFound => ("FOO_NOT_FOUND", NotFound),   // (クライアントに返すコード, ErrorType)
//!     }
//! );
//! parent_error!(
//!     pub enum BarUseCaseError {
//!         Foo(FooError),   // 末尾カンマ必須（マクロのパターンが `,` 区切りの繰り返しのため）
//!     }
//! );
//! ```
//!
//! 注意: マクロのパターンは `$vis enum` から始まるので、enum に `///` や `#[...]` は付けられない。

/// 下位エラーを variant として束ねる親エラーを定義する。`From<子>` も自動生成される。
#[macro_export]
macro_rules! parent_error {
    (
        $vis:vis enum $name:ident {
            $(
                $variant:ident($child:ty),
            )*
        }
    ) => {
        #[derive(Debug, PartialEq, Eq, Clone)]
        $vis enum $name {
            $(
                $variant($child),
            )*
        }

        impl $crate::common::traits::Cause for $name {
            fn cause(&self) -> Option<&str> {
                match self {
                    $(
                        Self::$variant(err) => err.cause(),
                    )*
                }
            }
        }

        impl $crate::common::traits::Code for $name {
            fn code(&self) -> &str {
                match self {
                    $(
                        Self::$variant(err) => err.code(),
                    )*
                }
            }
        }

        impl $crate::common::traits::ErrorTypeProvider for $name {
            fn error_type(&self) -> $crate::common::traits::ErrorType {
                match self {
                    $(
                        Self::$variant(err) => err.error_type(),
                    )*
                }
            }
        }

        $(
            impl ::std::convert::From<$child> for $name {
                fn from(value: $child) -> Self {
                    Self::$variant(value)
                }
            }
        )*
    };
}

/// 詳細メッセージを持たない末端エラーを定義する。`Variant => ("CODE", ErrorType)` の形で書く。
#[macro_export]
macro_rules! leaf_error {
    (
        $vis:vis enum $name:ident {
            $(
                $variant:ident => ($str:expr, $err:expr),
            )*
        }
    ) => {
        #[derive(Debug, PartialEq, Eq, Clone)]
        $vis enum $name {
            $(
                $variant,
            )*
        }

        impl $crate::common::traits::Code for $name {
            fn code(&self) -> &str {
                match self {
                    $(
                        Self::$variant => $str,
                    )*
                }
            }
        }

        impl $crate::common::traits::Cause for $name {
            fn cause(&self) -> Option<&str> {
                match self {
                    $(
                        Self::$variant => None,
                    )*
                }
            }
        }

        impl $crate::common::traits::ErrorTypeProvider for $name {
            fn error_type(&self) -> $crate::common::traits::ErrorType {
                match self {
                    $(
                        Self::$variant => $err,
                    )*
                }
            }
        }
    };
}

/// 詳細メッセージ（String）を持つ末端エラーを定義する。メッセージは `cause()` でログにだけ出る。
#[macro_export]
macro_rules! leaf_detail_error {
    (
        $vis:vis enum $name:ident {
            $(
                $variant:ident => ($str:expr, $err:expr),
            )*
        }
    ) => {
        #[derive(Debug, PartialEq, Eq, Clone)]
        $vis enum $name {
            $(
                $variant(String),
            )*
        }

        impl $crate::common::traits::Code for $name {
            fn code(&self) -> &str {
                match self {
                    $(
                        Self::$variant(_) => $str,
                    )*
                }
            }
        }

        impl $crate::common::traits::Cause for $name {
            fn cause(&self) -> Option<&str> {
                match self {
                    $(
                        Self::$variant(err) => Some(err),
                    )*
                }
            }
        }

        impl $crate::common::traits::ErrorTypeProvider for $name {
            fn error_type(&self) -> $crate::common::traits::ErrorType {
                match self {
                    $(
                        Self::$variant(_) => $err,
                    )*
                }
            }
        }
    };
}
