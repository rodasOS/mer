use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(Uuid);

        impl $name {
            /// Time-ordered id (UUID v7), so ids sort by creation.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn as_uuid(self) -> Uuid {
                self.0
            }

            /// Parses the text form (a UUID), e.g. from an environment variable.
            pub fn parse(text: &str) -> Option<Self> {
                Uuid::parse_str(text).ok().map(Self)
            }

            /// Rebuilds an id read back from storage.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

id_type!(
    /// Identifies a space.
    SpaceId
);
id_type!(
    /// Identifies a tab inside a space.
    TabId
);
id_type!(
    /// Identifies a terminal pane.
    PaneId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_parse_back_from_their_text_form() {
        let id = PaneId::new();
        assert_eq!(PaneId::parse(&id.as_uuid().to_string()), Some(id));
        assert_eq!(PaneId::parse("not-a-uuid"), None);
    }
}
