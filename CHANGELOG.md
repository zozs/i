# Changelog

### 2.1.0

* Fix bug where uploads larger than 2 MiB were denied.
* Add possibility to limit max upload size (default 2 GiB).

### 2.0.0

**Breaking changes**
* Logging is now done by using `tracing` instead, the old `--logger-format / -l` flag is now removed.

Other changes

* Move from actix-web to axum as web framework
* Better error handling

### 1.1.0

* Prevent empty (zero-length) uploads by returning 400 Bad Request

### 1.0.6

* Upgrade dependencies

### 1.0.5

* Upgrade dependencies

### 1.0.4

* Add example script for Wayland screenshot upload
* Upgrade dependencies

### 1.0.3

* No longer fail during delete if no thumbnail exists
* Upgrade dependencies

### 1.0.2

* Upgrade dependencies
* Build with Rust 1.68

### 1.0.1

* Upgrade dependencies

### 1.0.0

* Add support for deletion of files through UI
* Add slightly better looking 404 page
