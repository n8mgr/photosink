#ifndef AppVersion
  #error AppVersion must be supplied with /DAppVersion
#endif

[Setup]
AppId=com.n8mgr.photosink
AppName=Photosink
AppVersion={#AppVersion}
VersionInfoVersion={#GetVersionNumbersString(AddBackslash(SourcePath) + "bin\publish\win-x64\Photosink.exe")}
DefaultDirName={localappdata}\Programs\Photosink
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
UninstallDisplayIcon={app}\Photosink.exe
SetupIconFile=photosink.ico
LicenseFile=..\..\LICENSE
OutputDir=..\..\release
OutputBaseFilename=photosink_windows_x64_setup
Compression=lzma2
SolidCompression=yes
#ifdef Sign
SignTool=azure
SignedUninstaller=yes
#endif

[Files]
Source: "bin\publish\win-x64\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Photosink"; Filename: "{app}\Photosink.exe"
