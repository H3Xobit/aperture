CXX ?= g++
CXXFLAGS ?= -std=c++17 -O2 -Wall -Wextra -Werror -Iinclude
LDFLAGS ?= -pthread

SRC := src/aperture_limiter.cpp
TEST_BIN := tests/test_limiter

.PHONY: test clean

$(TEST_BIN): tests/test_limiter.cpp $(SRC) include/aperture_limiter.h
	$(CXX) $(CXXFLAGS) tests/test_limiter.cpp $(SRC) -o $@ $(LDFLAGS)

test: $(TEST_BIN)
	./$(TEST_BIN)

clean:
	rm -f $(TEST_BIN)
