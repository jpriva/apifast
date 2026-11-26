; The Windows installer of apifast, for Inno Setup 6: the interface
; (apifast-gui.exe) and the command line (apifast.exe), a Start menu entry
; and an uninstaller. Built by packaging\windows\build.ps1, which gives the
; version:
;
;   ISCC /DAppVersion=0.1.0 packaging\windows\apifast.iss
;
; It installs for the current user, without administrator rights, in
; %LOCALAPPDATA%\Programs\apifast; "for all users" is offered in a dialog and
; asks for them.
;
; Kept as UTF-8 with its byte order mark: without it, Inno Setup reads the
; Spanish texts in the system's code page.

#ifndef AppVersion
  #error AppVersion is not defined: build with packaging\windows\build.ps1
#endif

[Setup]
; Identifies apifast to Windows: an installer with the same id updates what
; is installed. Changing it breaks existing installations.
AppId={{2DF34EBF-574D-47C3-91EF-DAFFBB8297E1}
AppName=apifast
AppVersion={#AppVersion}
AppVerName=apifast {#AppVersion}
AppPublisher=tryxtek
AppPublisherURL=https://github.com/jpriva/apifast
AppSupportURL=https://github.com/jpriva/apifast/issues
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\apifast
DefaultGroupName=apifast
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The PATH task changes the environment: running programs are told.
ChangesEnvironment=yes
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
SetupIconFile=apifast.ico
UninstallDisplayIcon={app}\apifast-gui.exe
UninstallDisplayName=apifast
OutputDir=..\..\target\packages
OutputBaseFilename=apifast-{#AppVersion}-windows-x86_64-setup

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
en.AddToPath=Add the apifast command line to the PATH
es.AddToPath=Añadir la línea de comandos de apifast al PATH
en.CommandLine=Command line:
es.CommandLine=Línea de comandos:

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "path"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:CommandLine}"

[Files]
Source: "..\..\target\release\apifast-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\apifast.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\apifast"; Filename: "{app}\apifast-gui.exe"
Name: "{autodesktop}\apifast"; Filename: "{app}\apifast-gui.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\apifast-gui.exe"; Description: "{cm:LaunchProgram,apifast}"; Flags: nowait postinstall skipifsilent

[Code]
// The PATH of whoever apifast is installed for: the user's, or the
// machine's when installing for all users.
function EnvironmentRoot: Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function EnvironmentKey: String;
begin
  if IsAdminInstallMode then
    Result := 'System\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

// Adds the folder of apifast to the PATH, or removes it. It is compared
// without case and as a whole entry, so it is never added twice and no
// other entry is touched.
procedure SetInPath(Add: Boolean);
var
  Paths, Dir: String;
  At: Integer;
begin
  Dir := ExpandConstant('{app}');
  if not RegQueryStringValue(EnvironmentRoot, EnvironmentKey, 'Path', Paths) then
    Paths := '';
  // The entry's place in Paths: both are searched between semicolons.
  At := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';');
  if Add then
  begin
    if At > 0 then
      Exit;
    if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
      Paths := Paths + ';';
    Paths := Paths + Dir;
  end
  else
  begin
    if At = 0 then
      Exit;
    Delete(Paths, At, Length(Dir) + 1);
    if (Paths <> '') and (Paths[Length(Paths)] = ';') then
      Delete(Paths, Length(Paths), 1);
  end;
  // An expandable string, as Windows keeps it: entries like %USERPROFILE%
  // keep working.
  RegWriteExpandStringValue(EnvironmentRoot, EnvironmentKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('path') then
    SetInPath(True);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    SetInPath(False);
end;
