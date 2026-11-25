Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 32
Name "ZSDesk ${VERSION}"
OutFile "${OUTPUT}"
Icon "..\..\crates\zsdesk-app\resources\zsdesk.ico"
InstallDir "$LOCALAPPDATA\Programs\ZSDesk"
ShowInstDetails nevershow
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "nsDialogs.nsh"
!define REGKEY "Software\ZSDesk\Installer\Full"
!define LEGACY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{470DF6E5-CCB7-4A3F-9ED0-F68009D5A91A}_is1"
Var SetupLog
Var LogArg
Var OldDir
Var DesktopIcon
Var MainLogin
Var FenceLogin
Var LoginBits
Var MainChoice
Var FenceChoice
Var DesktopChoice
Var MenuChoice
!insertmacro MUI_PAGE_DIRECTORY
Page custom OptionsPage SaveOptions
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\ZSDesk.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"
LangString Unsupported ${LANG_SIMPCHINESE} "需要 Windows 10 1809 或更高版本（64 位）。"
LangString Unsupported ${LANG_ENGLISH} "Requires 64-bit Windows 10 version 1809 or later."
LangString Occupied ${LANG_SIMPCHINESE} "请选择空目录。已有旧版安装可使用对应的全量升级安装包。"
LangString Occupied ${LANG_ENGLISH} "Choose an empty directory. For an existing legacy installation, use the full upgrade installer."
LangString Busy ${LANG_SIMPCHINESE} "文件管理器仍在使用，请先完成文件操作。"
LangString Busy ${LANG_ENGLISH} "The file manager is busy. Finish active file operations before continuing."
LangString Failed ${LANG_SIMPCHINESE} "Windows Installer 安装失败，代码 $0。"
LangString Failed ${LANG_ENGLISH} "Windows Installer failed with code $0."
LangString Options ${LANG_SIMPCHINESE} "快捷方式与登录启动"
LangString Options ${LANG_ENGLISH} "Shortcuts and sign-in"
LangString DesktopLabel ${LANG_SIMPCHINESE} "创建桌面快捷方式"
LangString DesktopLabel ${LANG_ENGLISH} "Create a desktop shortcut"
LangString MainLabel ${LANG_SIMPCHINESE} "登录 Windows 时打开文件管理器"
LangString MainLabel ${LANG_ENGLISH} "Open the file manager when I sign in"
LangString FencesLabel ${LANG_SIMPCHINESE} "登录 Windows 时启动桌面格子"
LangString FencesLabel ${LANG_ENGLISH} "Start desktop fences when I sign in"
LangString StartupFailed ${LANG_SIMPCHINESE} "程序已安装。登录启动设置未能保存，可在应用设置中调整。"
LangString StartupFailed ${LANG_ENGLISH} "Installed. Sign-in settings could not be saved. You can change them in the application."
Function .onInit
  SetRegView 64
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastBuild} 17763
    MessageBox MB_ICONSTOP|MB_OK "$(Unsupported)" /SD IDOK
    SetErrorLevel 1633
    Abort
  ${EndIf}
  ReadRegStr $OldDir HKCU "${REGKEY}" "Path"
  StrCpy $DesktopChoice 0
  StrCpy $MenuChoice 1
  StrCpy $MainChoice "keep"
  StrCpy $FenceChoice "keep"
  StrCpy $LoginBits 16
  ${If} $OldDir != ""
    GetFullPathName $OldDir "$OldDir\."
    StrCpy $INSTDIR "$OldDir"
    ReadRegStr $0 HKCU "${REGKEY}" "DesktopLink"
    ${If} $0 == "1"
      StrCpy $DesktopChoice 1
    ${EndIf}
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} "$0" "/VERYSILENT" $1
  ${IfNot} ${Errors}
    SetSilent silent
  ${EndIf}
  ClearErrors
  ${GetOptions} "$0" "/DIR=" $1
  ${IfNot} ${Errors}
    StrCpy $INSTDIR "$1"
  ${EndIf}
  ClearErrors
  ${GetOptions} "$0" "/LOG=" $1
  ${IfNot} ${Errors}
    StrCpy $SetupLog "$1"
    StrCpy $LogArg '/L*v "$1"'
  ${EndIf}
  ClearErrors
  ${GetOptions} "$0" "/NOSHORTCUTS" $1
  ${IfNot} ${Errors}
    StrCpy $MenuChoice 0
    StrCpy $DesktopChoice 0
  ${EndIf}
FunctionEnd
Function OptionsPage
  !insertmacro MUI_HEADER_TEXT "$(Options)" "ZSDesk"
  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateCheckbox} 0 10u 100% 14u "$(DesktopLabel)"
  Pop $DesktopIcon
  ${NSD_CreateCheckbox} 0 36u 100% 14u "$(MainLabel)"
  Pop $MainLogin
  ${NSD_CreateCheckbox} 0 62u 100% 14u "$(FencesLabel)"
  Pop $FenceLogin
  ${If} $DesktopChoice == 1
    ${NSD_Check} $DesktopIcon
  ${EndIf}
  ${If} $OldDir != ""
    ExecWait '"$OldDir\ZSDesk.exe" --installer-startup-status "$OldDir"' $LoginBits
  ${EndIf}
  ${If} $LoginBits < 16
  ${OrIf} $LoginBits > 19
    EnableWindow $MainLogin 0
    EnableWindow $FenceLogin 0
  ${Else}
    IntOp $0 $LoginBits & 1
    ${If} $0 != 0
      ${NSD_Check} $MainLogin
    ${EndIf}
    IntOp $0 $LoginBits & 2
    ${If} $0 != 0
      ${NSD_Check} $FenceLogin
    ${EndIf}
  ${EndIf}
  nsDialogs::Show
FunctionEnd
Function SaveOptions
  ${NSD_GetState} $DesktopIcon $DesktopChoice
  ${If} $LoginBits >= 16
  ${AndIf} $LoginBits <= 19
    ${NSD_GetState} $MainLogin $0
    StrCpy $MainChoice "off"
    ${If} $0 == 1
      StrCpy $MainChoice "on"
    ${EndIf}
    ${NSD_GetState} $FenceLogin $0
    StrCpy $FenceChoice "off"
    ${If} $0 == 1
      StrCpy $FenceChoice "on"
    ${EndIf}
  ${EndIf}
FunctionEnd
Section
  ${If} $INSTDIR == ""
    Goto occupied
  ${EndIf}
  ${If} $OldDir != ""
  ${AndIf} $OldDir != $INSTDIR
    Goto occupied
  ${EndIf}
  ${If} $OldDir == ""
    IfFileExists "$INSTDIR\." 0 fresh
    StrCpy $2 ""
    FindFirst $1 $2 "$INSTDIR\*"
    ${DoWhile} $2 != ""
      ${If} $2 != "."
      ${AndIf} $2 != ".."
        FindClose $1
        Goto occupied
      ${EndIf}
      FindNext $1 $2
    ${Loop}
    FindClose $1
  ${EndIf}
  fresh:
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  File /r "${PAYLOAD}\*"
  File "/oname=ZSDesk.msi" "${MSI}"
  IfFileExists "$INSTDIR\ZSDesk.exe" 0 install
    ExecWait '"$OUTDIR\ZSDesk.exe" --shutdown-for-update "$INSTDIR\ZSDesk.exe"' $0
    ${If} $0 != 0
      MessageBox MB_ICONSTOP|MB_OK "$(Busy)" /SD IDOK
      SetErrorLevel 1618
      Abort
    ${EndIf}
  install:
  StrCpy $3 ""
  ReadRegStr $1 HKCU "${REGKEY}" "ProductCode"
  ${If} $1 == "${PRODUCT}"
    StrCpy $3 "REINSTALL=ALL REINSTALLMODE=vamus"
  ${EndIf}
  ExecWait '"$SYSDIR\msiexec.exe" /i "$OUTDIR\ZSDesk.msi" /qn /norestart INSTALLDIR="$INSTDIR" MENUSHORTCUTS=$MenuChoice DESKTOPICON=$DesktopChoice $3 $LogArg' $0
  ${If} $0 != 0
  ${AndIf} $0 != 3010
    MessageBox MB_ICONSTOP|MB_OK "$(Failed)" /SD IDOK
    SetErrorLevel $0
    Abort
  ${EndIf}
  ${If} $0 == 3010
    SetRebootFlag true
  ${EndIf}
  ${If} $MainChoice != "keep"
  ${OrIf} $FenceChoice != "keep"
    ExecWait '"$INSTDIR\ZSDesk.exe" --installer-startup-apply $MainChoice $FenceChoice' $0
    ${If} $0 != 0
      MessageBox MB_ICONEXCLAMATION|MB_OK "$(StartupFailed)" /SD IDOK
    ${EndIf}
  ${EndIf}
  Goto done
  occupied:
  ${If} $SetupLog != ""
    FileOpen $4 "$SetupLog" a
    FileWrite $4 "Destination check: previous=[$OldDir] destination=[$INSTDIR] entry=[$2]$\r$\n"
    FileClose $4
  ${EndIf}
  MessageBox MB_ICONSTOP|MB_OK "$(Occupied)" /SD IDOK
  SetErrorLevel 1638
  Abort
  done:
SectionEnd
