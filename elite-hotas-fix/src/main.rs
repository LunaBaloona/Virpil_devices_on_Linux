//! elite-hotas-fix: keep flight sticks and throttles working in Elite Dangerous
//! (or  technically any Steam game) running under Proton on Linux.
//!
//! The problem it solves:: when a Proton version treats a flight controller as an
//! Xbox-style-amepad, Wine records the device in the prefix registry with an
//! "&IG_00" suffix. Those records stay behind after you switch Proton versions,
//! and the game keeps seeing the stick as an XInput pad ("XB360 Pad") instead of
//! a DirectInput joystick, so its bindings stop working.
