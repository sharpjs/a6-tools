#!/usr/bin/env ruby
#
# a6pkg - A6 Software Update Packager/Unpackager
#
# Copyright (C) 2015 Jeffrey Sharp
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.

require 'optparse'

module A6
  class Packager
    def run(args)
      @options = parse_options(args)
      send(@options[:mode] || :decode)
    end

    def parse_options(args)
      {}.tap do |options|
        OptionParser.new do |p|
          p.banner = "usage: a6pkg [options] [file ...]"

          p.on("-d", "--decode", "Convert SysEx to binary (default).") do
            options[:mode] = :decode
          end

          p.on("--decode-sysex", "Convert SysEx to blocks.") do
            options[:mode] = :decode_sysex
          end

          p.on("-h", "--help", "Print this help message.") do
            puts p
            exit
          end

          # Future work
          #o.on("--decode-blocks", "Convert blocks to binary")
          #o.on("-e", "--encode", "Convert binary to SysEx")
          #o.on("--encode-binary", "Convert binary to blocks")
          #o.on("--encode-blocks", "Convert blocks to SysEx")
        end.parse!(args)
      end
    end

    def decode
      sysex_decoder = SysexDecoder.new
      block_decoder = BlockDecoder.new

      each_file do |sysex|
        sysex_decoder.decode(sysex) do |block|
          block_decoder.decode(block)
        end
      end

      block_decoder.verify.write_to($stdout)
    end

    def decode_sysex
      sysex_decoder = SysexDecoder.new
      blocks = ''.b

      each_file do |sysex|
        sysex_decoder.decode(sysex) do |block|
          blocks << block
        end
      end

      $stdout << blocks
    end

    protected

    def each_file
      ARGF.binmode
      loop do
        begin
          yield ARGF.file.read
        rescue FormatError => e
          warn "#{ARGF.filename}: #{e}"
          exit 1
        end
        ARGF.skip
        break if ARGV.empty?
      end
    end
  end

  class FormatError < StandardError
  end

  class SysexDecoder
    attr_reader :kind

    def decode(midi)
      first_op = nil

      midi.scan(SYSEX) do |op, data|
        offset = $~.begin(0)

        if !first_op
          case first_op = op
          when Op::UPDATE_OS   then @kind = :os
          when Op::UPDATE_BOOT then @kind = :boot
          end
        elsif op != first_op
          raise FormatError, 'sysex offset %d: expected %02X, but found %02X' % [
              offset + 5, first_op, op
          ]
        end

        yield data_decode(data)
      end
      self
    end

    private

    def data_decode(midi)
      "".b.tap do |data|
        offset = 0
        bits8  = 0
        midi.each_byte do |bits7|
          n = offset % 8
          offset += 1
          if n == 0
            bits8 = bits7
          else
            bits8 |= bits7 << (8 - n)
            data << (bits8 & 0xFF)
            bits8 >>= 8
          end
        end
      end
    end

    # A6 SysEx Frame Format
    SYSEX = /
      \xF0            # SysEx start
      \x00\x00\x0E    # Manufacturer ID
      \x1D            # Family ID
      ([\x30\x3F])    # Opcode
      ([\x00-\x7F]*+) # Data
      \xF7            # SysEx end
    /xn

    module Op
      UPDATE_OS   = "\x30"
      UPDATE_BOOT = "\x3F"
    end
  end

  class BlockDecoder
    attr_reader :version, :checksum, :length, :blocks

    def initialize
      @blocks = []
      @offset = 0
      @sum    = 0
    end

    def decode(block)
      unless block.length == BLOCK_LEN
        invalid 0, "expected block of #{BLOCK_LEN} bytes, but got #{block.length} bytes"
      end

      # Read header
      version, checksum, length, block_count, block_index =
        block.unpack('L>3S>2')

      if @offset == 0
        @blocks = Array.new(block_count)
        @version, @checksum, @length, @block_count =
         version,  checksum,  length,  block_count
      else
        require_same  0, 'version',     version,     @version
        require_same  4, 'checksum',    checksum,    @checksum
        require_same  8, 'length',      length,      @length
        require_same 12, 'block count', block_count, @block_count
      end

      unless (0...block_count) === block_index
        invalid 14, "block index #{block_index} is out of range"
      end

      if @blocks[block_index]
        invalid 14, "duplicate block #{block_index}"
      end

      length = [[@length - @offset, DATA_LEN].min, 0].max
      block  = block[HEADER_LEN, length]

      @blocks[block_index] = block
      @offset += length
      @sum     = block.each_byte.reduce(@sum, :+) & 0xFFFFFFFF
      self
    end

    def verify
      nil_index = @blocks.find_index(nil) and raise FormatError,
        "one or more blocks missing, starting at block #{nil_index}"

      @checksum == @sum or raise FormatError,
        "header checksum 0x#{'%08X' % @checksum} "\
        "does not match calculated sum 0x#{'%08X' % @sum}"

      self
    end

    def write_to(io)
      @blocks.reduce(io, :<<)
      self
    end

    def write_metadata_to(io)
      io.puts \
        "Version:  #{format_version(version)}",
        "Checksum: #{'%08X' % checksum}",
        "Length:   #{'%8d'  % length} bytes",
        "Blocks:   #{'%8d'  % blocks.count} 256-byte blocks"
      self
    end

    private

    HEADER_LEN =  16 # bytes
    DATA_LEN   = 256 # bytes
    BLOCK_LEN  = HEADER_LEN + DATA_LEN

    def format_version(v)
      "#{v / 10000}.#{v / 100 % 100}.#{v % 100}"
    end

    def require_same(offset, property, actual, expected)
      unless actual == expected
        invalid offset, "#{property} does not match previous blocks"
      end
    end

    def invalid(offset, reason)
      raise FormatError, "at blocks offset #{@offset + offset}: #{reason}"
    end
  end
end

if __FILE__ == $0
  # Running as a script
  A6::Packager.new.run(ARGV)
end

# vim: sw=2 sts=2
