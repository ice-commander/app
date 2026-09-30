!include "MUI2.nsh"
!include "x64.nsh"

Name "Ice Commander Console"
OutFile "..\\..\\distr\\ice-commander-console-0.8.130-1-win64.exe"
InstallDir "$PROGRAMFILES64\Ice Commander Console"
Target amd64-unicode

SetCompressor /SOLID lzma

RequestExecutionLevel admin

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\ice-console.exe"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "Polish"
!insertmacro MUI_LANGUAGE "Czech"
!insertmacro MUI_LANGUAGE "Slovak"
!insertmacro MUI_LANGUAGE "German"
!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro MUI_LANGUAGE "Ukrainian"
!insertmacro MUI_LANGUAGE "Italian"
!insertmacro MUI_LANGUAGE "French"
!insertmacro MUI_LANGUAGE "Romanian"
!insertmacro MUI_LANGUAGE "Hungarian"
!insertmacro MUI_LANGUAGE "Belarusian"
!insertmacro MUI_LANGUAGE "Bulgarian"
!insertmacro MUI_LANGUAGE "Serbian"

Function .onInit
    ${If} ${RunningX64}
        SetRegView 64
        StrCpy $INSTDIR "$PROGRAMFILES64\Ice Commander Console"
    ${Else}
        MessageBox MB_OK|MB_ICONSTOP "This program requires 64-bit Windows / Эта программа требует 64-битную версию Windows."
        Abort
    ${EndIf}
FunctionEnd

Section "Ice Commander Console (Required)" SecMain
    SectionIn RO
    SetOutPath "$INSTDIR"

    File "..\..\bin\distr\exe\target\x86_64-pc-windows-gnu\release\ice-console.exe"

    CreateDirectory "$SMPROGRAMS\Ice Commander Console"
    CreateShortcut "$SMPROGRAMS\Ice Commander Console\Ice Commander Console.lnk" "$INSTDIR\ice-console.exe" "" "$INSTDIR\ice-console.exe" 0
    CreateShortcut "$SMPROGRAMS\Ice Commander Console\Uninstall Ice Commander Console.lnk" "$INSTDIR\Uninstall.exe"

    WriteUninstaller "$INSTDIR\Uninstall.exe"

    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole" "DisplayName" "Ice Commander Console"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole" "DisplayIcon" '"$INSTDIR\ice-console.exe"'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole" "DisplayVersion" "0.8.130"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole" "Publisher" "Ice Commander Project"
SectionEnd

Section /o "Create Desktop Shortcut" SecDesktop
    CreateShortcut "$DESKTOP\Ice Commander Console.lnk" "$INSTDIR\ice-console.exe"
SectionEnd

Section "Uninstall"
	SetRegView 64

    ExecWait 'taskkill /F /IM ice-console.exe'

    RMDir /r "$INSTDIR"

    Delete "$SMPROGRAMS\Ice Commander Console\Ice Commander Console.lnk"
    Delete "$SMPROGRAMS\Ice Commander Console\Uninstall Ice Commander Console.lnk"
    RMDir "$SMPROGRAMS\Ice Commander Console"
    Delete "$DESKTOP\Ice Commander Console.lnk"

    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\IceCommanderConsole"
SectionEnd
