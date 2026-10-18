# The GTK stack (android-rust-gtk4-adwaita) in an app's Android build script: paste or source.

# Sets and exports GTK_STACK, so that Gradle run from the same script takes the same folder: the
# GTK_STACK environment variable, a checkout for work on the stack (relative to the lock's folder,
# with the ABIs built), else the release gtk-stack.lock pins, fetched into the shared cache when it
# is not there.
#   resolve_gtk_stack <fetch-gtk-stack.sh> <gtk-stack.lock> [abi...]
resolve_gtk_stack() {
	local fetch="$1" lock="$2" abi arch dir args=()
	shift 2
	if [ -n "${GTK_STACK:-}" ]; then
		dir="$(cd "$(dirname "$lock")" && cd "$GTK_STACK" 2>/dev/null && pwd)" ||
			{ echo "GTK_STACK=$GTK_STACK: no such folder" >&2; return 1; }
		for abi in "$@"; do
			case "$abi" in
				arm64-v8a) arch=aarch64 ;;
				armeabi-v7a) arch=armv7 ;;
				*) echo "unknown ABI $abi (arm64-v8a, armeabi-v7a)" >&2; return 1 ;;
			esac
			[ -d "$dir/bin/gtk-stack/root-$arch/lib/$abi/pkgconfig" ] || {
				echo "GTK_STACK=$dir has no $abi stack (bin/gtk-stack/root-$arch): build it there" \
					"(npm run build-gtk-stack), or unset GTK_STACK for the release gtk-stack.lock pins" >&2
				return 1
			}
		done
	else
		for abi in "$@"; do args+=(--abi "$abi"); done
		dir="$("$fetch" "${args[@]}" "$lock")" || return 1
	fi
	export GTK_STACK="$dir"
}

# --remap-path-prefix for ~/.cargo, ~/.rustup, the project (the folder above every local crate) and
# CARGO_TARGET_DIR when it is elsewhere: no build-machine paths in panic locations. For any target's
# rustflags, desktop builds too; space-separated, so these paths must have no spaces.
#   rust_remap_flags <project dir>
rust_remap_flags() {
	local project
	project="$(cd "$1" && pwd)"
	printf '%s' "--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
		" --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup --remap-path-prefix=$project=."
	case "${CARGO_TARGET_DIR:-}" in
		"" | "$project"/*) ;;
		*) printf ' %s' "--remap-path-prefix=$(realpath -m "$CARGO_TARGET_DIR")=/target" ;;
	esac
}

# The same for C and C++ that the cc crate builds (sources in ~/.cargo/registry, __FILE__).
#   c_prefix_map_flags <project dir>
c_prefix_map_flags() {
	local project
	project="$(cd "$1" && pwd)"
	printf '%s' "-ffile-prefix-map=${CARGO_HOME:-$HOME/.cargo}=/cargo -ffile-prefix-map=$project=."
}

# Per-target environment for cargo ndk -t <abi> against GTK_STACK. The -sys crates' pkg-config is
# scoped to the target (build scripts on the host keep the host's); its paths name the stack's
# version, so a new version re-runs them (pkg-config's rerun-if-env-changed).
#   gtk_stack_cargo_env <abi> <project dir>
gtk_stack_cargo_env() {
	local abi="$1" project="$2" arch triple var
	case "$abi" in
		arm64-v8a) arch=aarch64 triple=aarch64-linux-android ;;
		armeabi-v7a) arch=armv7 triple=armv7-linux-androideabi ;;
		*) echo "unknown ABI $abi (arm64-v8a, armeabi-v7a)" >&2; return 1 ;;
	esac
	# Either replaces target.<triple>.rustflags (16 KB pages in .cargo/config.toml) and the remapping.
	if [ -n "${RUSTFLAGS:-}${CARGO_ENCODED_RUSTFLAGS:-}" ]; then
		echo "RUSTFLAGS is set: cargo would ignore target.$triple.rustflags; unset it" >&2
		return 1
	fi
	var="${triple//-/_}"
	export PKG_CONFIG_ALLOW_CROSS=1
	export "PKG_CONFIG_SYSROOT_DIR_$var=$GTK_STACK/bin/gtk-stack/root-$arch"
	export "PKG_CONFIG_LIBDIR_$var=$GTK_STACK/bin/gtk-stack/root-$arch/lib/$abi/pkgconfig"
	# cargo appends this to target.<triple>.rustflags from .cargo/config.toml; cargo ndk (4.x) adds its
	# --target to the C flags.
	export "CARGO_TARGET_${var^^}_RUSTFLAGS=$(rust_remap_flags "$project")"
	export "CFLAGS_$var=$(c_prefix_map_flags "$project")"
	export "CXXFLAGS_$var=$(c_prefix_map_flags "$project")"
}

# Build-machine paths left in built libraries; prints file: count, returns 1 if any.
#   check_build_paths <file>...
check_build_paths() {
	local f n bad=0
	for f in "$@"; do
		n="$(strings -a "$f" | grep -cE "/development/|$HOME/|/tmp/claude" || true)"
		[ "$n" = 0 ] || { echo "$f: $n build-machine paths" >&2; bad=1; }
	done
	return "$bad"
}
