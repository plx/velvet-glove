package helper

import "os"

func RemoveWorkspaceFile() {
	os.Remove("workspace-only")
}
