package fixture

import (
	"fmt"
	"testing"
)

func TestInternal(t *testing.T) {
	_ = t
	fmt.Printf("%d\n", "internal-test")
}
