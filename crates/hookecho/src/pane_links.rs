//! Spatial link membership travels with a pane; dimensions can use different groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialLinks {
    pub camera: Link,
    pub site: Link,
    pub cursor: Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub group: u8,
    pub enabled: bool,
}
impl Default for Link {
    fn default() -> Self {
        Self {
            group: 1,
            enabled: false,
        }
    }
}
impl Link {
    pub fn valid(self) -> bool {
        (1..=crate::view::MAX_PANES as u8).contains(&self.group)
    }
    pub fn shares(self, other: Self) -> bool {
        self.enabled && other.enabled && self.group == other.group
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Camera,
    Site,
    Cursor,
}
impl Dimension {
    pub const ALL: [Self; 3] = [Self::Camera, Self::Site, Self::Cursor];
    pub fn label(self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Site => "Radar site",
            Self::Cursor => "Geographic cursor",
        }
    }
}
impl SpatialLinks {
    pub fn legacy(camera: bool, site: bool, cursor: bool) -> Self {
        Self {
            camera: Link {
                enabled: camera,
                ..Default::default()
            },
            site: Link {
                enabled: site,
                ..Default::default()
            },
            cursor: Link {
                enabled: cursor,
                ..Default::default()
            },
        }
    }
    pub fn get(self, dimension: Dimension) -> Link {
        match dimension {
            Dimension::Camera => self.camera,
            Dimension::Site => self.site,
            Dimension::Cursor => self.cursor,
        }
    }
    pub fn get_mut(&mut self, dimension: Dimension) -> &mut Link {
        match dimension {
            Dimension::Camera => &mut self.camera,
            Dimension::Site => &mut self.site,
            Dimension::Cursor => &mut self.cursor,
        }
    }
    pub fn valid(self) -> bool {
        Dimension::ALL.into_iter().all(|d| self.get(d).valid())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSpatialLinks {
    pub schema: u8,
    pub links: SpatialLinks,
}
impl SavedSpatialLinks {
    pub fn valid(self) -> bool {
        self.schema == 1 && self.links.valid()
    }
}
