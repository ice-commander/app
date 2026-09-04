!include "MUI2.nsh"
!include "x64.nsh"

Name "Ice Commander"
OutFile "..\\..\\distr\\ice-commander-0.7.122-1-win64.exe"
InstallDir "$PROGRAMFILES64\Ice Commander"
Target amd64-unicode

SetCompressor /SOLID lzma

RequestExecutionLevel admin

!define MUI_ICON "assets\win32-icon.ico"
!define MUI_UNICON "assets\win32-icon.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\ice-commander.exe"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Russian"

Function .onInit
    ${If} ${RunningX64}
        SetRegView 64
        StrCpy $INSTDIR "$PROGRAMFILES64\Ice Commander"
    ${Else}
        MessageBox MB_OK|MB_ICONSTOP "This program requires 64-bit Windows / Эта программа требует 64-битную версию Windows."
        Abort
    ${EndIf}
FunctionEnd

Section "Ice Commander (Required)" SecMain
    SectionIn RO ; Read Only - cannot be deselected
    SetOutPath "$INSTDIR"
    
    File "..\..\bin\distr\exe\target\x86_64-pc-windows-gnu\release\ice-commander.exe"
    
    ; liblzo2-2.dll is excluded on purpose: it is GPL-2.0-or-later and reaches the bundle only
    ; through libgtk-4 -> libcairo-script-interpreter, a debug path the application never uses.
    ; fakelzo (see src/fakelzo/README.md) supplies the two symbols the interpreter imports instead.
    ; Excluding it here — rather than overwriting it after install — keeps the GPL library out
    ; of the installer archive entirely, which is what actually matters for distribution.
    File /r /x liblzo2-2.dll "..\..\artifacts\gtk4-win32-x64\*"
    File "..\..\bin\distr\fakelzo\liblzo2-2.dll"

    ; The bundle ships LGPL libraries (the GTK stack, libmpv and FFmpeg); their license
    ; texts and the source references must accompany it.
    SetOutPath "$INSTDIR\licenses"
    File "..\..\assets\licenses\*.txt"
    SetOutPath "$INSTDIR"
    
    CreateDirectory "$SMPROGRAMS\Ice Commander"
    CreateShortcut "$SMPROGRAMS\Ice Commander\Ice Commander.lnk" "$INSTDIR\ice-commander.exe" "" "$INSTDIR\ice-commander.exe" 0
    CreateShortcut "$SMPROGRAMS\Ice Commander\Uninstall Ice Commander.lnk" "$INSTDIR\Uninstall.exe"
    
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander" "DisplayName" "Ice Commander"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander" "DisplayIcon" '"$INSTDIR\ice-commander.exe"'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander" "DisplayVersion" "0.7.122"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander" "Publisher" "Ice Commander Project"
SectionEnd

Section /o "Create Desktop Shortcut" SecDesktop
    CreateShortcut "$DESKTOP\Ice Commander.lnk" "$INSTDIR\ice-commander.exe"
SectionEnd

Section "Uninstall"
	SetRegView 64

    ExecWait 'taskkill /F /IM ice-commander.exe'
    
    RMDir /r "$INSTDIR"
    
    Delete "$SMPROGRAMS\Ice Commander\Ice Commander.lnk"
    Delete "$SMPROGRAMS\Ice Commander\Uninstall Ice Commander.lnk"
    RMDir "$SMPROGRAMS\Ice Commander"
    Delete "$DESKTOP\Ice Commander.lnk"
    
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommander"
SectionEnd
