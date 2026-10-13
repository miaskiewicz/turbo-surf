# Standalone experiment: use installed Apple Command Line Tools without Xcode.
import subprocess,json
for key,command in [('sdk_path','--show-sdk-path'),('sdk_version','--show-sdk-version'),('sdk_build','--show-sdk-build-version')]:
 print(key+' = '+json.dumps(subprocess.check_output(['xcrun',command],text=True).strip()))
print('xcode_version = "0000"\nxcode_version_int = 0\nxcode_build = "CLT"')
