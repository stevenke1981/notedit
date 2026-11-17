#ifndef AppVersion
  #error AppVersion must be supplied by scripts/package-windows.ps1
#endif
#ifndef RuntimeDir
  #error RuntimeDir must be supplied by scripts/package-windows.ps1
#endif

[Setup]
AppId={{C118C6F8-7D8F-405B-9515-A718D3C34BCC}
AppName=NotEdit
AppVersion={#AppVersion}
AppPublisher=NotEdit
DefaultDirName={localappdata}\Programs\NotEdit
DefaultGroupName=NotEdit
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\release\installer
OutputBaseFilename=NotEdit-{#AppVersion}-windows-x64-setup
SetupIconFile=..\ui\icon.ico
UninstallDisplayIcon={app}\notedit.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "contextmenu"; Description: "在檔案總管右鍵選單加入 NotEdit（Windows 11：顯示其他選項）"; Flags: checkedonce
Name: "desktopicon"; Description: "建立桌面捷徑"; Flags: unchecked

[Files]
Source: "..\target\release\notedit.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\notedit_spell.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RuntimeDir}\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\dict\LICENSE-en_US.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ui\fonts\LICENSE-DejaVu.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ui\fonts\LICENSE-JetBrainsMono-OFL.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\NotEdit"; Filename: "{app}\notedit.exe"
Name: "{autodesktop}\NotEdit"; Filename: "{app}\notedit.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "用 NotEdit 開啟"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%1"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "用 NotEdit 開啟資料夾"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%1"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "用 NotEdit 開啟資料夾"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%V"""; Tasks: contextmenu

[Run]
Filename: "{app}\notedit.exe"; Description: "啟動 NotEdit"; Flags: nowait postinstall skipifsilent
