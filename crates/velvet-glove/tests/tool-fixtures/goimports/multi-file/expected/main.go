package main

import (
	"fmt"

	externalwidget "example.net/external/widget"
	"example.test/root/widget"
)

func main() { fmt.Println(widget.Value, externalwidget.Value) }
