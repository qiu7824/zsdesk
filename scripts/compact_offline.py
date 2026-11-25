"""Build a per-user MSI and wrap its complete external source in a solid NSIS installer.

The build host needs 64-bit CPython 3.8–3.12 (msilib) and Unicode NSIS 3.
The target uses Windows Installer and does not need Python, NSIS, or a network.
"""
import argparse
import hashlib
import json
import re
import struct
import subprocess
import uuid
from pathlib import Path

import msilib
from msilib import schema, sequence

NAMESPACE = uuid.UUID("b229c60a-0d02-4469-b43b-7be01ea2968f")
REGKEY = r"Software\ZSDesk\Installer\Full"
def guid(value):
    return "{"+str(uuid.uuid5(NAMESPACE,value)).upper()+"}"

def identifier(prefix, name):
    return prefix+hashlib.sha256(name.encode("utf-8")).hexdigest()[:24]

def build(payload, work, compiler):
    work.mkdir(parents=True,exist_ok=True)
    manifest=json.loads((payload/"manifest.json").read_text(encoding="utf-8-sig"))
    version=manifest["version"]
    if not msilib.AMD64 or manifest["edition"]!="full" or not re.fullmatch(r"\d+\.\d+\.\d+",version):
        raise ValueError("An x64 full-edition payload and 64-bit Python are required")
    members=sorted([*manifest["files"],"manifest.json"])
    if manifest.get("search_engine") is not None or any(
        name.lower().startswith("engines/everything/") or re.fullmatch(r"everything(?:32|64)?\.(?:exe|dll|lng)",Path(name).name,re.I)
        for name in members
    ):
        raise ValueError("Select an external-only search payload")
    if set(members)!={p.relative_to(payload).as_posix() for p in payload.rglob("*") if p.is_file()}:
        raise ValueError("Payload inventory differs from the release manifest")
    product=guid("full/"+version)
    msi=work/"ZSDesk.msi"
    db=msilib.init_database(str(msi),schema,"ZSDesk "+version+" Full",product,version,"ZSDesk contributors")
    codepage=work/"codepage.idt"
    codepage.write_bytes(b"\r\n\r\n936\t_ForceCodepage\r\n")
    del db
    import ctypes as ct
    from ctypes import wintypes as wt
    windows_installer=ct.WinDLL("msi")
    windows_installer.MsiOpenDatabaseW.argtypes=[wt.LPCWSTR,ct.c_void_p,ct.POINTER(wt.DWORD)]
    windows_installer.MsiDatabaseImportW.argtypes=[wt.DWORD,wt.LPCWSTR,wt.LPCWSTR]
    handle=wt.DWORD()
    def checked(error):
        if error: raise ct.WinError(error)
    checked(windows_installer.MsiOpenDatabaseW(str(msi),1,ct.byref(handle)))
    try:
        checked(windows_installer.MsiDatabaseImportW(handle,str(work),codepage.name))
        checked(windows_installer.MsiDatabaseCommit(handle))
    finally:
        windows_installer.MsiCloseHandle(handle)
    db=msilib.OpenDatabase(str(msi),msilib.MSIDBOPEN_TRANSACT)
    summary=db.GetSummaryInformation(3)
    summary.SetProperty(msilib.PID_WORDCOUNT,8) # uncompressed external source, long names, per user
    summary.SetProperty(msilib.PID_PAGECOUNT,500)
    summary.Persist()
    def rows(table,*records):
        msilib.add_data(db,table,list(records))
    for table in sequence.tables:
        records=getattr(sequence,table)
        if table=="InstallExecuteSequence":
            records=[r for r in records if r[0]!="RemoveExistingProducts"]
        msilib.add_data(db,table,records)
    rows("Property",("UpgradeCode",guid("full/upgrade")),("ARPNOMODIFY","1"),("ARPNOREPAIR","1"),
         ("SecureCustomProperties","OLDPRODUCTS;NEWERPRODUCT;INSTALLDIR;DESKTOPICON;MENUSHORTCUTS"))
    rows("Upgrade",(guid("full/upgrade"),None,version,None,0,None,"OLDPRODUCTS"),
         (guid("full/upgrade"),version,None,None,2,None,"NEWERPRODUCT"))
    rows("LaunchCondition",("NOT NEWERPRODUCT","A newer version of ZSDesk is installed."))
    rows("InstallExecuteSequence",("RemoveExistingProducts","OLDPRODUCTS",1501))
    rows("Directory",("TARGETDIR",None,"SourceDir"),("LocalAppDataFolder","TARGETDIR","."),
         ("ProgramsDir","LocalAppDataFolder","Programs:."),("INSTALLDIR","ProgramsDir","ZSDesk:."),
         ("ProgramMenuFolder","TARGETDIR","."),("MenuDir","ProgramMenuFolder","ZSDesk"),
         ("DesktopFolder","TARGETDIR","."))
    rows("Feature",("Main",None,"ZSDesk","File manager, desktop fences and search",1,1,None,0))
    dirs={"":"INSTALLDIR"}
    for index,name in enumerate(members,1):
        relative=Path(name)
        file=(payload/relative).resolve()
        if payload not in file.parents: raise ValueError("Invalid package path")
        data=file.read_bytes()
        if name!="manifest.json" and hashlib.sha256(data).hexdigest()!=manifest["files"][name]["sha256"]:
            raise ValueError("Payload hash mismatch: "+name)
        parent=""
        for segment in relative.parts[:-1]:
            current=(parent+"/"+segment).lstrip("/")
            if current not in dirs:
                dirs[current]=identifier("D",current)
                rows("Directory",(dirs[current],dirs[parent],"D%06X|%s"%(len(dirs),segment)))
            parent=current
        fid=identifier("F",name); component=identifier("C",name)
        rows("Component",(component,guid("file/"+name),dirs[parent],256,None,fid))
        rows("FeatureComponents",("Main",component))
        short="F%06X"%index
        rows("File",(fid,component,short+"|"+relative.name,len(data),None,None,512|8192,index))
        rows("MsiFileHash",(fid,0,*struct.unpack("<iiii",hashlib.md5(data).digest())))
    rows("Media",(1,len(members),None,None,None,None))
    rows("Component",("Settings",guid("registration"),"INSTALLDIR",260,None,"InstallPath"))
    rows("FeatureComponents",("Main","Settings"))
    rows("Registry",("InstallPath",1,REGKEY,"Path","[INSTALLDIR]","Settings"),
         ("InstallProduct",1,REGKEY,"ProductCode","[ProductCode]","Settings"))
    app=identifier("F","ZSDesk.exe")
    for component,folder,condition,items in [
        ("MenuLinks","MenuDir",'MENUSHORTCUTS <> "0"',[("MainLink","ZSDesk",None),("FencesLink","FENCES|ZSDesk Desktop Fences","--desktop")]),
        ("DesktopLink","DesktopFolder",'DESKTOPICON = "1"',[("DeskLink","ZSDesk",None)])]:
        rows("Component",(component,guid(component),folder,260,condition,component))
        rows("FeatureComponents",("Main",component))
        rows("Registry",(component,1,REGKEY,component,"1",component))
        for key,label,args in items:
            rows("Shortcut",(key,folder,label,component,"[#"+app+"]",args,None,None,None,None,1,"INSTALLDIR"))
    rows("RemoveFile",("CleanMenu","MenuLinks",None,"MenuDir",2))
    rows("CustomAction",("RestoreIntegration",18,app,"--uninstall-integration"))
    rows("InstallExecuteSequence",("RestoreIntegration",'REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE',3150))
    db.Commit()
    del summary,db
    output=work/("ZSDesk-"+version+"-full-x64-offline-setup.exe")
    script=Path(__file__).resolve().parents[1]/"packaging/windows/compact-full.nsi"
    subprocess.run([str(compiler),"/INPUTCHARSET","UTF8","/V2","/DPAYLOAD="+str(payload),"/DMSI="+str(msi),
                    "/DPRODUCT="+product,"/DVERSION="+version,"/DOUTPUT="+str(output),str(script)],check=True)
    print(output,output.stat().st_size,flush=True)
    if output.stat().st_size>=5_000_000:
        raise ValueError("Full offline installer exceeds 5,000,000 bytes")

if __name__=="__main__":
    args=argparse.ArgumentParser(description=__doc__)
    for name in ["payload","work","compiler"]: args.add_argument("--"+name,required=True,type=Path)
    opt=args.parse_args()
    build(opt.payload.resolve(),opt.work.resolve(),opt.compiler.resolve())
