package main

import (
	"bytes"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestPrintVersion_TextEmitsBareVersion(t *testing.T) {
	var buf bytes.Buffer
	require.NoError(t, printVersion(&buf, "text"))
	assert.Equal(t, version+"\n", buf.String())
}

func TestPrintVersion_JSONEmitsNameAndVersion(t *testing.T) {
	var buf bytes.Buffer
	require.NoError(t, printVersion(&buf, "json"))
	var got versionInfo
	require.NoError(t, json.Unmarshal(buf.Bytes(), &got))
	assert.Equal(t, versionInfo{Name: "taskloom", Version: version}, got)
}

func TestPrintVersion_UnknownFormatErrors(t *testing.T) {
	var buf bytes.Buffer
	err := printVersion(&buf, "yaml")
	require.Error(t, err)
	assert.Empty(t, buf.String())
}
