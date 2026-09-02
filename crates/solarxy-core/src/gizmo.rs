//! What a viewport manipulator is allowed to write, and under what names.
//!
//! Lives here rather than beside either of its two users because neither may
//! depend on the other: the engine fills one of these in from a node's
//! descriptor, and the host's drag solver asks it which parameter a handle
//! writes. That is the same reason `scene` lives here.
//!
//! The type exists because a parameter's ROLE and its NAME are different
//! facts, and only the node knows the second one. A `geo` names its position
//! `translate`, a point light names the same role `position`, and a rect-area
//! light names it `translate` again while naming its size `width` and
//! `height` rather than a scale lane. Identifying a target by node type and
//! writing a name chosen at compile time are the same mistake seen twice, and
//! both get more expensive with every node that ought to be manipulable. A
//! target that carries its own names costs the same for one node type as for
//! ten.
//!
//! `None` is load-bearing everywhere in here: it means the node does not
//! declare that role at all, and a handle that would write it must neither
//! draw nor grab. It is not a default to fall back from. Reading an
//! undeclared key is not a benign miss either, because the resolver
//! debug-asserts on a key its descriptor never declared, so the failure mode
//! of guessing is a debug-build panic rather than a quiet no-op.

/// The parameters one manipulable node declares, by name.
///
/// Every field is a name the node's own descriptor declares, so a value read
/// out of here can be handed to a parameter write without further checking.
/// A node that declares none of them is not manipulable and produces no
/// target at all rather than an empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransformParams {
    /// The position, whatever the node calls it.
    pub translate: Option<&'static str>,
    /// Euler angles in degrees.
    pub rotate: Option<&'static str>,
    /// The enum naming the order [`Self::rotate`] composes in. `None` where
    /// the node fixes an order rather than exposing one, which a rect-area
    /// light does: its angles are always XYZ.
    pub rotate_order: Option<&'static str>,
    /// How this node says how big it is, which is not always a scale.
    pub scale: ScaleParams,
    /// The point rotation and scale happen about, in the node's own space.
    /// Only a `transform` declares one.
    pub pivot: Option<&'static str>,
    /// A position the node points AT, rather than an orientation it carries.
    /// Aiming is not rotating, and calling it rotating would be a small lie
    /// with consequences: a spot light has no orientation to decompose, it
    /// has a second point in space.
    pub aim: Option<&'static str>,
}

impl TransformParams {
    /// Every parameter this target's transform is made of, in a stable order.
    ///
    /// What "reset this transform" means, and what a shell tells its frontend
    /// the selection's transform consists of. One definition, so resetting a
    /// panel resets exactly the params its handles write and no others.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        names.extend(self.translate);
        names.extend(self.rotate);
        names.extend(self.rotate_order);
        match self.scale {
            ScaleParams::None => {}
            ScaleParams::Vec3 { scale, uniform } => names.extend([scale, uniform]),
            ScaleParams::Extent2 { x, z } => names.extend([x, z]),
        }
        names.extend(self.pivot);
        names.extend(self.aim);
        names
    }
}

/// How a node says how big it is.
///
/// Two shapes rather than one because they are genuinely different writes,
/// not one write with a different name: a scale is a dimensionless multiplier
/// on three lanes plus a uniform factor, and an extent is a pair of
/// independent lengths in metres. Collapsing them would mean either scaling a
/// light by a factor it does not store or storing a geometry scale in metres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleParams {
    /// The node has no size of its own. A point light is a point.
    #[default]
    None,
    /// Three scale lanes and a uniform multiplier over them, which is what a
    /// `geo` and a `transform` carry.
    Vec3 {
        scale: &'static str,
        uniform: &'static str,
    },
    /// Two edge lengths in metres along the node's own X and Z, which is what
    /// a rect-area light's width and height are. There is deliberately no Y:
    /// a panel has no thickness, so a handle for one would write nowhere.
    Extent2 { x: &'static str, z: &'static str },
}
