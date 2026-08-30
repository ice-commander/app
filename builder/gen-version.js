const fs = require('fs');
const path = require('path');

const args = process.argv.slice(2);
if (args.length < 2) {
	console.error('Usage: node gen-version.js <app_type> <build_type>');
	console.error('Example: node gen-version.js gui deb');
	process.exit(1);
}

const app_type = args[0];
const build_type = args[1];

const packageJsonPath = path.join(__dirname, '..', 'package.json');
const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
const version = packageJson.version;

const outPath = path.join(
	__dirname,
	'..',
	'src',
	'common',
	'common',
	'src',
	'version.rs',
);

const content = `pub const APP_VERSION: &str = "${version}";
pub const APP_TYPE: &str = "${app_type}";
pub const BUILD_TYPE: &str = "${build_type}";
`;

fs.writeFileSync(outPath, content, 'utf8');

console.log(`Generated version.rs -> v${version} [${app_type}:${build_type}]`);

// -------------------------------------------------------------
// Automatically Synchronize Version across Installers and Cargo
// -------------------------------------------------------------

function replaceTomlVersion(content, sectionHeader, newVersion) {
	const lines = content.split('\n');
	let inSection = false;
	let modified = false;

	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();

		if (line.startsWith('[')) {
			inSection = line === sectionHeader;
		} else if (inSection && line.startsWith('version')) {
			const parts = lines[i].split('=');
			if (parts.length >= 2 && parts[0].trim() === 'version') {
				const leftSide = lines[i].substring(
					0,
					lines[i].indexOf('=') + 1,
				);
				lines[i] = `${leftSide} "${newVersion}"`;
				modified = true;
				break; // Only replace the first one in the section
			}
		}
	}
	return { content: lines.join('\n'), modified };
}

// Sync a crate's [package] version to the single source of truth (package.json), so every
// package type ships ONE version (deb/rpm read Cargo.toml, arch reads package.json).
function syncCargoVersion(cargoPath, newVersion) {
	if (!fs.existsSync(cargoPath)) return;
	const original = fs.readFileSync(cargoPath, 'utf8');
	const { content, modified } = replaceTomlVersion(original, '[package]', newVersion);
	if (modified && content !== original) {
		fs.writeFileSync(cargoPath, content, 'utf8');
		console.log(`Synced ${path.relative(path.join(__dirname, '..'), cargoPath)} -> v${newVersion}`);
	}
}

function updateSetupNsi(nsiPath, outFileName) {
	if (fs.existsSync(nsiPath)) {
		let content = fs.readFileSync(nsiPath, 'utf8');
		let modified = false;

		const regStrRegex =
			/(WriteRegStr\s+HKLM\s+"[^"]+"\s+"DisplayVersion"\s+)"[^"]+"/g;
		if (regStrRegex.test(content)) {
			content = content.replace(regStrRegex, `$1"${version}"`);
			modified = true;
		}

		const outFileRegex = /^(OutFile\s+)"[^"]+"/m;
		if (outFileRegex.test(content)) {
			content = content.replace(
				outFileRegex,
				`$1"..\\\\..\\\\distr\\\\${outFileName}"`,
			);
			modified = true;
		}

		if (modified) {
			fs.writeFileSync(nsiPath, content, 'utf8');
			console.log(
				`Auto-synced OutFile and DisplayVersion in setup.nsi -> v${version}`,
			);
		}
	}
}

function updateBuildGradle(gradlePath) {
	if (fs.existsSync(gradlePath)) {
		let content = fs.readFileSync(gradlePath, 'utf8');
		let modified = false;

		const vNameRegex = /versionName\s*=\s*"[^"]+"/;
		if (vNameRegex.test(content)) {
			content = content.replace(vNameRegex, `versionName = "${version}"`);
			modified = true;
		}

		const vCodeRegex = /versionCode\s*=\s*\d+/;
		if (vCodeRegex.test(content)) {
			const vParts = version.split('.');
			let code = 1;
			if (vParts.length >= 3) {
				// Example: 0.1.174 -> 1174
				code =
					parseInt(vParts[0]) * 1000000 +
					parseInt(vParts[1]) * 1000 +
					parseInt(vParts[2]);
			}
			content = content.replace(vCodeRegex, `versionCode = ${code}`);
			modified = true;
		}

		if (modified) {
			fs.writeFileSync(gradlePath, content, 'utf8');
			console.log(`Auto-synced Android build.gradle.kts -> v${version}`);
		}
	}
}

// The NSIS installer script + its output name, per app type.
const setupByType = {
	gui: {
		nsi: path.join(__dirname, '..', 'src', 'gtk-app', 'setup.nsi'),
		out: `ice-commander-${version}-1-win64.exe`,
	},
	webserver: {
		nsi: path.join(__dirname, '..', 'src', 'webserver-app', 'setup.nsi'),
		out: `ice-commander-webserver-${version}-1-win64.exe`,
	},
	console: {
		nsi: path.join(__dirname, '..', 'src', 'console-app', 'setup.nsi'),
		out: `ice-commander-console-${version}-1-win64.exe`,
	},
};
if (setupByType[app_type]) {
	updateSetupNsi(setupByType[app_type].nsi, setupByType[app_type].out);
}

// Unify package versions: point the built crate's Cargo.toml at package.json's version, so
// its deb/rpm match the arch package (and each other) — one version across everything.
const cargoByType = {
	gui: path.join(__dirname, '..', 'src', 'gtk-app', 'Cargo.toml'),
	webserver: path.join(__dirname, '..', 'src', 'webserver-app', 'Cargo.toml'),
	console: path.join(__dirname, '..', 'src', 'console-app', 'Cargo.toml'),
};
if (cargoByType[app_type]) {
	syncCargoVersion(cargoByType[app_type], version);
}
