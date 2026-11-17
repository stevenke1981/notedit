; Built by IronSetup (https://github.com/stevenke1981/ironsetup), a strict Inno .iss subset.
; AppVersion and RuntimeDir are supplied on the command line: /DAppVersion=x.y.z /DRuntimeDir=...
; (see scripts/package-windows.ps1).

[Setup]
AppId={{C118C6F8-7D8F-405B-9515-A718D3C34BCC}
AppName=NotEdit
AppVersion={#AppVersion}
AppPublisher=NotEdit
DefaultDirName={localappdata}\Programs\NotEdit
DefaultGroupName=NotEdit
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
OutputDir=..\target\release\installer
OutputBaseFilename=NotEdit-{#AppVersion}-windows-x64-setup
SetupIconFile=..\ui\icon.ico
UninstallDisplayIcon={app}\notedit.exe
Compression=zip

[Tasks]
Name: "contextmenu"; Description: "在檔案總管右鍵選單加入 NotEdit（Windows 11：顯示其他選項）"
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
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "使用 NotEdit 開啟"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\*\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%1"""; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "使用 NotEdit 開啟資料夾"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%1"""; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit"; ValueType: string; ValueName: ""; ValueData: "使用 NotEdit 開啟資料夾"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\notedit.exe"",0"; Tasks: contextmenu; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\NotEdit\command"; ValueType: string; ValueName: ""; ValueData: """{app}\notedit.exe"" ""%V"""; Tasks: contextmenu; Flags: uninsdeletevalue

[Run]
Filename: "{app}\notedit.exe"; Description: "啟動 NotEdit"; Flags: nowait postinstall skipifsilent
