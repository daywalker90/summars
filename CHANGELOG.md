# Changelog

## [Unreleased]

### Added

- ``summars-pays-columns`` See README for more info
- ``summars-max-description-length`` Set the max length for descriptions, see README
- ``summars-invoices-columns`` See README for more info
- ``summars-max-label-length`` Set the max length for invoice labels, see README

### Changed

- Options code refactored. All options are now natively dynamic and there is no longer any manual reading of config files. Read the updated README section on how to set options for more information
- ``summars-max-alias-length`` supports wrapping by using negative values (just like the new ``summars-max-description-length`` and ``summars-max-label-length``), see README
- ``summars-columns`` and the new ``-columns`` options now also dictate the order of the columns

## [3.2.0] - 2024-05-03

### Added

- `summars-sort-by`: now supports reverse sorting by using a `-` prefix, e.g. `-ALIAS`
- `summars-json`: new boolean option to output data in json format

### Changed

- if you had the plugin with config file options start with CLN and then changed an option and only reloaded the plugin, CLN would pass stale option values to the plugin so the load priority changed to:
    1. config file options
    2. ``plugin start`` options

### Fixed

- no longer ignore general config file if a network config file exists