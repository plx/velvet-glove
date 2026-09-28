package sub

import "os"

func Remove() {
	os.Remove("foo")
}
