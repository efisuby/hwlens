% hwlens(1) | User Commands
%
% 2025-05-01

# NAME

hwlens - Terminal system monitor for processor, GPU, memory, disks, network, sensors and processes.

# SYNOPSIS

**hwlens** [**-c** _file_] [**-d**] [**-f** _filter_] [**-l**] [**-p** _id_] [**-t**] [**-u** _ms_] [**\-\-force-utf**] [**\-\-socket** _path_] [**\-\-themes-dir** _dir_]

**hwlens** [**\-\-default-config** | {**-h** | **\-\-help**} | {**-V** | **\-\-version**}]

# DESCRIPTION

**hwlens** is a program that shows usage and stats for processor, memory, disks, network, and processes.

The data comes from **hwlensd**, a system service that hwlens connects to through
_/run/hwlensd/hwlensd.sock_. Members of the **hwlens** group may connect.

# OPTIONS

The program follows the usual GNU command line syntax, with long options
starting with two dashes ('-'). A summary of options is included below.

**-c**, **\-\-config _file_**
:   Path to a config file.

**-d**, **\-\-debug**
:   Start in debug mode with additional logs and metrics.

**-f**, **\-\-filter _filter_**
:   Set an initial process filter.

**\-\-force-utf**
:   Force start even if no UTF-8 locale was detected.

**-l**, **\-\-low-color**
:   Disable true color, 256 colors only.

**-p**, **\-\-preset _id_**
:   Start with a preset (0-9).

**-t**, **\-\-tty**
:   Force tty mode with ANSI graph symbols and 16 colors only.

**\-\-no-tty**
:   Force disable tty mode.

**\-\-socket _path_**
:   Socket of hwlensd. The default is _/run/hwlensd/hwlensd.sock_, or **HWLENSD_SOCKET** when set.

**\-\-themes-dir _dir_**
:   Path to a custom themes directory. When specified, this directory takes priority over the default theme search paths.

**-u**, **\-\-update _ms_**
:   Set an initial update rate in milliseconds.

**\-\-default-config**
:   Print default config to standard output.

**-h**, **\-\-help**
:   Show summary of options.

**-V**, **\-\-version**
:   Show version of program.

# BUGS

The upstream bug tracker can be found at https://github.com/efisuby/hwlens/issues.

# SEE ALSO

**top**(1), **htop**(1), **systemctl**(1)

# AUTHOR

**hwlens** was written by Jakob P. Liljenberg a.k.a. "Aristocratos".
